use pyo3::prelude::*;
use tokio::runtime::Runtime;

#[pyclass]
pub struct HMIRSession {
    runtime: Runtime,
}

#[pymethods]
impl HMIRSession {
    #[new]
    fn new(_model_path: &str, _strategy: &str) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { runtime })
    }

    fn generate(&mut self, _prompt: &str, _max_tokens: usize) -> PyResult<String> {
        self.runtime.block_on(async {
            Ok(String::from("Verified draft string response natively bounded via GIL constraints"))
        })
    }
}

#[pymodule]
fn hmir(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<HMIRSession>()?;
    Ok(())
}
