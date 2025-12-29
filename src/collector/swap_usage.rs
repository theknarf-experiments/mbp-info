use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use super::{bytes_to_mb, Collector, MAX_SAMPLES};

pub struct SwapUsageCollector;

impl SwapUsageCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for SwapUsageCollector {
    fn id(&self) -> &'static str {
        "swap_usage"
    }

    fn name(&self) -> &'static str {
        "Swap Usage"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let usage = read_swap_usage()?;

        let entries = vec![
            ParsedDatum {
                key: "Swap Used|MB".to_string(),
                label: "Swap Used".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(usage.xsu_used)),
            },
            ParsedDatum {
                key: "Swap Free|MB".to_string(),
                label: "Swap Free".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(usage.xsu_avail)),
            },
            ParsedDatum {
                key: "Swap Total|MB".to_string(),
                label: "Swap Total".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(usage.xsu_total)),
            },
        ];

        Ok(SampleData { entries })
    }
}

fn read_swap_usage() -> Result<libc::xsw_usage, String> {
    use libc::c_void;
    use std::ffi::CString;

    let mut usage: libc::xsw_usage = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::xsw_usage>();
    let cname = CString::new("vm.swapusage").map_err(|_| "invalid sysctl name".to_string())?;

    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut usage as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err("sysctl vm.swapusage failed".to_string());
    }

    Ok(usage)
}
