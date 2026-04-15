// hmir-core/src/topology/draft_policy.rs

#[derive(PartialEq)]
pub enum DraftPolicy {
    Aggressive,
    BatteryAware,
    ContextSensitive,
}

#[derive(PartialEq)]
pub enum PowerState {
    OnBattery,
    PluggedIn,
}

pub struct SpeculativeConfig {
    pub depth: usize,
    pub target_placement: u8,
}

pub struct HardwareAwareDraftSelector;

impl HardwareAwareDraftSelector {
    #[cfg(feature = "battery-aware")]
    pub fn should_draft(
        &self,
        policy: DraftPolicy,
        power_state: PowerState,
        context_len: usize,
        npu_available: bool,
    ) -> bool {
        match policy {
            DraftPolicy::BatteryAware if power_state == PowerState::OnBattery => {
                // If on battery, forcefully optimize tokens/watt over tokens/sec thresholds!
                // Estimate Power Saved (T_compute over NPU vs T_compute GPU) > 0.3x margin
                npu_available && context_len < 32000
            }
            DraftPolicy::ContextSensitive if context_len > 16384 => false,
            _ => npu_available,
        }
    }

    #[cfg(not(feature = "battery-aware"))]
    pub fn should_draft(
        &self,
        policy: DraftPolicy,
        _power_state: PowerState,
        context_len: usize,
        npu_available: bool,
    ) -> bool {
        match policy {
            DraftPolicy::ContextSensitive if context_len > 16384 => false,
            _ => npu_available,
        }
    }
}
