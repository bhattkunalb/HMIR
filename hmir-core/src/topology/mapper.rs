/// Defines the user's operational intent for the current session.
#[derive(Debug, PartialEq, Eq)]
pub enum ComputeIntent {
    /// Optimize for Time-To-First-Token and minimal wall-clock completion
    Latency,
    /// Optimize for max batch size (tokens/sec total) regardless of single user latency
    Throughput,
    /// Extreme limits on power envelope; highly favors NPU and efficiency CPU cores
    Battery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceType {
    NPU,
    GPU,
    CPU,
}

/// Abstract representation of an available physical accelerator
#[derive(Debug, Clone)]
pub struct HardwareDevice {
    pub id: String,
    pub device_type: DeviceType,
    pub effective_tflops: f64,
    pub tdp_watts: f64,
}

impl HardwareDevice {
    pub fn is_efficiency_npu(&self) -> bool {
        self.device_type == DeviceType::NPU && self.tdp_watts < 15.0
    }
}

/// Hardcoded Cost Matrix mimicking the ACPI / hwloc probing Phase 1 objective.
/// Holds the PCIe/Interconnect transfer speeds between nodes (GB/s).
pub struct CostMatrix {
    // Map of (FromDevice, ToDevice) -> Bandwidth in GB/s
    bandwidths: std::collections::HashMap<(String, String), f64>,
}

impl CostMatrix {
    pub fn new() -> Self {
        Self {
            bandwidths: std::collections::HashMap::new(),
        }
    }

    pub fn set_bandwidth(&mut self, from: &str, to: &str, gbps: f64) {
        self.bandwidths.insert((from.to_string(), to.to_string()), gbps);
    }

    pub fn get_bandwidth(&self, from: &str, to: &str) -> f64 {
        // Fallback to minimal cross-socket PCIe gen3 x4 speed (4 GB/s) if unknown
        *self.bandwidths.get(&(from.to_string(), to.to_string())).unwrap_or(&4.0)
    }
}

/// The core explicit transfer-cost routing decision module (from the Pseudocode specs).
pub struct Router;

impl Router {
    /// Decides if moving the tensor to a target device performs faster than computing locally, 
    /// explicitly factoring in the power intent and exact transfer bandwidth.
    pub fn should_route_to_accelerator(
        transfer_bytes: f64,
        flops_required: f64,
        target_device: &HardwareDevice,
        current_device: &HardwareDevice,
        topology_matrix: &CostMatrix,
        intent: &ComputeIntent,
    ) -> bool {
        
        let bw_gbps = topology_matrix.get_bandwidth(&current_device.id, &target_device.id);
        
        // Convert to GB for time calc
        let transfer_gb = transfer_bytes / 1_000_000_000.0;
        let t_transfer = transfer_gb / bw_gbps;

        let t_compute_target = flops_required / target_device.effective_tflops;
        let t_compute_current = flops_required / current_device.effective_tflops;
        
        // Battery/Power guardrail: Force NPU usage if battery intent selected and NPU is available
        if *intent == ComputeIntent::Battery && target_device.is_efficiency_npu() {
            // Check if power saved validates the (potentially slower) processing
            let power_diff = current_device.tdp_watts - target_device.tdp_watts;
            let power_saved_joules = power_diff * t_compute_current;
            if power_saved_joules > 50.0 { // 50 Joules threshold
                return true;
            }
        }

        // Standard Latency Equation
        let t_total_target = t_transfer + t_compute_target;
        
        // Only route if transferring AND computing takes LESS time than just computing locally.
        t_total_target < t_compute_current
    }
}

// -----------------------------------------------------------------------------
// TESTS
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_devices() -> (HardwareDevice, HardwareDevice, CostMatrix) {
        let cpu = HardwareDevice {
            id: "CPU_0".to_string(),
            device_type: DeviceType::CPU,
            effective_tflops: 2.0, // CPU slow 
            tdp_watts: 65.0,
        };

        let gpu = HardwareDevice {
            id: "GPU_0".to_string(),
            device_type: DeviceType::GPU,   
            effective_tflops: 40.0, // GPU fast
            tdp_watts: 320.0,
        };

        let mut matrix = CostMatrix::new();
        // Assume PCIe Gen4 x16 -> ~32 GB/s
        matrix.set_bandwidth("CPU_0", "GPU_0", 32.0); 

        (cpu, gpu, matrix)
    }

    #[test]
    fn test_latency_routing_decision() {
        let (cpu, gpu, matrix) = setup_devices();

        let intent = ComputeIntent::Latency;
        
        // Scenario 1: Heavy compute, small transfer (Should offload to GPU)
        // Tensor: 1 GB (1,000,000,000 bytes)
        // FLOPS req: 200 TFLOPS
        let should_route = Router::should_route_to_accelerator(
            1_000_000_000.0,
            200.0,
            &gpu,
            &cpu,
            &matrix,
            &intent
        );
        
        // Computation on CPU: 200 / 2.0 = 100 seconds
        // Transfer + Compute GPU: (1.0 / 32.0) + (200 / 40.0) = 0.03 + 5.0 = 5.03 seconds
        // 5.03 < 100 -> Route to GPU!
        assert!(should_route, "Should offload to GPU when compute heavy.");
        
        // Scenario 2: Tiny compute, massive transfer (Stay on CPU)
        // Tensor: 20 GB 
        // FLOPS: 1 TFLOP
        let should_route_heavy_transfer = Router::should_route_to_accelerator(
            20_000_000_000.0,
            1.0,
            &gpu,
            &cpu,
            &matrix,
            &intent
        );
        
        // CPU: 1 / 2.0 = 0.5s
        // GPU: (20.0 / 32.0) + (1 / 40) = 0.625 + 0.025 = 0.650s
        // 0.650 NOT < 0.5 -> Do NOT route! Stay on CPU.
        assert!(!should_route_heavy_transfer, "Should stay on CPU when transfer penalty is higher than compute saving.");
    }

    #[test]
    fn test_battery_intent_override() {
        let cpu = HardwareDevice {
            id: "CPU_0".to_string(),
            device_type: DeviceType::CPU,
            effective_tflops: 15.0,
            tdp_watts: 100.0,
        };

        let npu = HardwareDevice {
            id: "NPU_0".to_string(),
            device_type: DeviceType::NPU,   
            effective_tflops: 5.0, // NPU is slower than this heavy CPU
            tdp_watts: 10.0,       // But highly efficient
        };

        let mut matrix = CostMatrix::new();
        matrix.set_bandwidth("CPU_0", "NPU_0", 16.0); // Unified memory buffer speed

        // 100 TFLOP workload
        // CPU time: 100 / 15.0 = 6.6s
        // NPU time: 100 / 5.0 = 20.0s
        // Under Latency intent, we would STAY on CPU.
        let latency_route = Router::should_route_to_accelerator(
            1_000_000_000.0, 100.0, &npu, &cpu, &matrix, &ComputeIntent::Latency
        );
        assert!(!latency_route, "Latency intent stays on CPU because NPU is slower.");

        // Under Battery intent, we should override and offload to NPU to save joules
        let battery_route = Router::should_route_to_accelerator(
            1_000_000_000.0, 100.0, &npu, &cpu, &matrix, &ComputeIntent::Battery
        );
        assert!(battery_route, "Battery intent should override and push to NPU.");
    }
}
