use thiserror::Error;

/// Unified error type for `speed-precise-core` DirectInput8, device enumeration,
/// live calibration, Force Feedback (`Step Log 2`), LUT mathematics, and simulator injection.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("DirectInput8 API call `{context}` failed with HRESULT 0x{hresult:08X}")]
    DirectInput { context: &'static str, hresult: u32 },

    #[error("Controller at index {index} not found (total connected controllers: {total})")]
    DeviceIndexOutOfRange { index: usize, total: usize },

    #[error("Axis with ID/index `{axis_id}` not found on device `{device_name}`")]
    AxisNotFound { axis_id: u32, device_name: String },

    #[error("Device `{device_name}` does not support DirectInput8 Force Feedback (DIDC_FORCEFEEDBACK)")]
    NoFfbSupport { device_name: String },

    #[error("Invalid Step Log 2 configuration: {reason}")]
    InvalidStepLogConfig { reason: &'static str },

    #[error("Step Log 2 execution aborted by user (Emergency FFB Cutoff triggered)")]
    UserAborted,

    #[error("Invalid calibration bounds: min ({min}), center ({center}), max ({max}) must satisfy min <= center <= max and min < max")]
    InvalidCalibrationBounds { min: i32, center: i32, max: i32 },

    #[error("Invalid property value for `{property}`: {value} (expected {min}..={max})")]
    InvalidPropertyRange {
        property: &'static str,
        value: u32,
        min: u32,
        max: u32,
    },

    #[error("Failed to persist calibration backup to `{path}`: {source}")]
    BackupIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to write Step Log 2 CSV to `{path}`: {source}")]
    CsvIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse WheelCheck CSV at line {line}: {reason}")]
    CsvParse { line: usize, reason: String },

    #[error("WheelCheck CSV contains no valid data samples")]
    EmptyStepLogCsv,

    #[error("WheelCheck CSV has zero maximum displacement (wheel did not move during test)")]
    ZeroMaxDisplacement,

    #[error("Invalid LUT generation configuration: {reason}")]
    InvalidLutConfig { reason: &'static str },

    #[error("Failed to read/write LUT file `{path}`: {source}")]
    LutIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to inject simulator profile at `{path}`: {source}")]
    SimInjectIo {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to serialize calibration backup JSON: {0}")]
    BackupJson(#[from] serde_json::Error),
}

pub type CoreResult<T> = Result<T, CoreError>;

impl CoreError {
    #[inline]
    pub fn from_hresult(context: &'static str, hr: i32) -> Self {
        Self::DirectInput {
            context,
            hresult: hr as u32,
        }
    }
}

/// Checks a COM `HRESULT` (`< 0` means failure) and maps it into [`CoreError`].
#[inline]
pub fn check_hresult(context: &'static str, hr: i32) -> CoreResult<()> {
    if hr < 0 {
        Err(CoreError::from_hresult(context, hr))
    } else {
        Ok(())
    }
}
