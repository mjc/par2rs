//! Shared command-line compatibility parsing.

use log::LevelFilter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseLevel {
    Silent,
    Quiet,
    Normal,
    Noisy,
    Debug,
}

impl NoiseLevel {
    fn log_filter(self) -> LevelFilter {
        match self {
            NoiseLevel::Silent => LevelFilter::Off,
            NoiseLevel::Quiet => LevelFilter::Error,
            NoiseLevel::Normal => LevelFilter::Warn,
            NoiseLevel::Noisy => LevelFilter::Info,
            NoiseLevel::Debug => LevelFilter::Debug,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkipScanOptions {
    pub data_skipping: bool,
    pub skip_leeway: usize,
}

pub fn parse_noise_level(verbose_count: u8, quiet_count: u8) -> Result<NoiseLevel, String> {
    if verbose_count > 0 && quiet_count > 0 {
        return Err("Verbose and quiet options cannot be used together".to_string());
    }

    if quiet_count >= 2 {
        Ok(NoiseLevel::Silent)
    } else if quiet_count == 1 {
        Ok(NoiseLevel::Quiet)
    } else if verbose_count >= 2 {
        Ok(NoiseLevel::Debug)
    } else if verbose_count == 1 {
        Ok(NoiseLevel::Noisy)
    } else {
        Ok(NoiseLevel::Normal)
    }
}

/// Parse a memory limit expressed in MB and return bytes.
pub fn parse_memory_mb(value: Option<&str>) -> Result<Option<usize>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let mb = value
        .parse::<usize>()
        .map_err(|_| format!("Invalid memory value: {value}"))?;
    if mb == 0 {
        return Err("Memory value must be greater than 0".to_string());
    }
    mb.checked_mul(1024 * 1024)
        .map(Some)
        .ok_or_else(|| "Memory value is too large".to_string())
}

pub fn parse_positive_usize(value: Option<&str>, flag_name: &str) -> Result<Option<usize>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("Invalid {flag_name} value: {value}"))?;
    if parsed == 0 {
        return Err(format!("{flag_name} must be greater than 0"));
    }
    Ok(Some(parsed))
}

pub fn parse_skip_options(
    data_skipping: bool,
    skip_leeway: Option<&str>,
) -> Result<SkipScanOptions, String> {
    let skip_leeway = match (data_skipping, skip_leeway) {
        (false, Some(_)) => {
            return Err("-S/--skip-leeway is only valid when -N data skipping is enabled".into());
        }
        (true, Some(value)) => parse_positive_usize(Some(value), "-S")?.unwrap(),
        (true, None) => 64,
        (false, None) => 0,
    };

    Ok(SkipScanOptions {
        data_skipping,
        skip_leeway,
    })
}

pub fn init_env_logger(noise_level: NoiseLevel) {
    let mut builder = env_logger::Builder::from_default_env();
    builder
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false);

    if std::env::var_os("RUST_LOG").is_none() {
        builder.filter_level(noise_level.log_filter());
    }

    let _ = builder.try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_level_matches_turbo_counts() {
        assert_eq!(parse_noise_level(0, 0).unwrap(), NoiseLevel::Normal);
        assert_eq!(parse_noise_level(1, 0).unwrap(), NoiseLevel::Noisy);
        assert_eq!(parse_noise_level(2, 0).unwrap(), NoiseLevel::Debug);
        assert_eq!(parse_noise_level(0, 1).unwrap(), NoiseLevel::Quiet);
        assert_eq!(parse_noise_level(0, 2).unwrap(), NoiseLevel::Silent);
        assert!(parse_noise_level(1, 1).is_err());
    }

    #[test]
    fn memory_rejects_zero() {
        assert_eq!(parse_memory_mb(None).unwrap(), None);
        assert_eq!(parse_memory_mb(Some("1")).unwrap(), Some(1024 * 1024));
        assert!(parse_memory_mb(Some("0")).is_err());
    }

    #[test]
    fn positive_usize_rejects_zero() {
        assert_eq!(parse_positive_usize(None, "-T").unwrap(), None);
        assert_eq!(parse_positive_usize(Some("2"), "-T").unwrap(), Some(2));
        assert!(parse_positive_usize(Some("0"), "-T").is_err());
    }

    #[test]
    fn skip_options_require_data_skipping() {
        assert_eq!(
            parse_skip_options(true, None).unwrap(),
            SkipScanOptions {
                data_skipping: true,
                skip_leeway: 64
            }
        );
        assert_eq!(
            parse_skip_options(true, Some("10")).unwrap(),
            SkipScanOptions {
                data_skipping: true,
                skip_leeway: 10
            }
        );
        assert!(parse_skip_options(false, Some("10")).is_err());
    }
}
