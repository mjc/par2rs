//! Shared command-line compatibility parsing.

use log::LevelFilter;
use std::ffi::OsString;

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

pub fn reject_detached_short_values<I>(args: I, attached_only_flags: &[&str]) -> Result<(), String>
where
    I: IntoIterator<Item = OsString>,
{
    reject_short_value_forms(args, attached_only_flags, &[])
}

pub fn reject_short_value_forms<I>(
    args: I,
    detached_rejected_flags: &[&str],
    equals_rejected_flags: &[&str],
) -> Result<(), String>
where
    I: IntoIterator<Item = OsString>,
{
    for arg in args {
        if arg == "--" {
            break;
        }
        let arg = arg.to_string_lossy();
        if detached_rejected_flags
            .iter()
            .any(|flag| arg.as_ref() == *flag)
        {
            return Err(format!(
                "{arg} requires an attached value for par2cmdline compatibility"
            ));
        }
        if equals_rejected_flags.iter().any(|flag| {
            arg.strip_prefix(*flag)
                .is_some_and(|suffix| suffix.starts_with('='))
        }) {
            return Err(format!(
                "{arg} is not a supported par2cmdline-compatible option form"
            ));
        }
    }
    Ok(())
}

pub fn normalize_mixed_noise_option_clusters<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let expanded = args
        .into_iter()
        .flat_map(expand_thread_option_noise_cluster)
        .collect::<Vec<_>>();

    expanded
        .into_iter()
        .map(|arg| normalize_mixed_noise_option_cluster(&arg).unwrap_or(arg))
        .collect()
}

fn expand_thread_option_noise_cluster(arg: OsString) -> Vec<OsString> {
    split_thread_option_noise_cluster(&arg)
        .map(|(thread_arg, noise_arg)| vec![thread_arg, noise_arg])
        .unwrap_or_else(|| vec![arg])
}

fn split_thread_option_noise_cluster(arg: &OsString) -> Option<(OsString, OsString)> {
    let arg_text = arg.to_str()?;
    let cluster = arg_text.strip_prefix('-')?;
    if cluster.is_empty() || cluster.starts_with('-') {
        return None;
    }

    let option = cluster.chars().next()?;
    if option != 't' && option != 'T' {
        return None;
    }

    let remainder = &cluster[option.len_utf8()..];
    let digit_count = remainder
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .count();
    if digit_count == 0 || digit_count == remainder.len() {
        return None;
    }

    let (value, tail_cluster) = remainder.split_at(digit_count);
    if !tail_cluster
        .chars()
        .all(|ch| matches!(ch, 'q' | 'v' | 'p' | 'O' | 'N' | 'u' | 'l'))
    {
        return None;
    }

    Some((
        OsString::from(format!("-{option}{value}")),
        OsString::from(format!("-{tail_cluster}")),
    ))
}

fn normalize_mixed_noise_option_cluster(arg: &OsString) -> Option<OsString> {
    let arg_text = arg.to_str()?;
    let cluster = arg_text.strip_prefix('-')?;
    if cluster.is_empty()
        || cluster.starts_with('-')
        || !cluster.chars().all(|ch| ch == 'q' || ch == 'v')
        || !cluster.contains('q')
        || !cluster.contains('v')
    {
        return None;
    }

    let first = cluster.chars().next()?;
    let count = cluster.chars().take_while(|ch| *ch == first).count();
    Some(OsString::from(format!(
        "-{}",
        std::iter::repeat_n(first, count).collect::<String>()
    )))
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

    #[test]
    fn detached_short_value_rejection_stops_at_terminator() {
        assert!(
            reject_detached_short_values(["-b", "8"].into_iter().map(OsString::from), &["-b"])
                .is_err()
        );
        assert!(
            reject_detached_short_values(["-b8"].into_iter().map(OsString::from), &["-b"]).is_ok()
        );
        assert!(reject_detached_short_values(
            ["--", "-b"].into_iter().map(OsString::from),
            &["-b"]
        )
        .is_ok());
    }

    #[test]
    fn mixed_noise_clusters_normalize_to_initial_run() {
        let normalized = normalize_mixed_noise_option_clusters(
            ["par2", "create", "-qv", "-vvq", "-qq", "--quiet"]
                .into_iter()
                .map(OsString::from),
        );
        let as_text: Vec<_> = normalized
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            as_text,
            vec!["par2", "create", "-q", "-vv", "-qq", "--quiet"]
        );
    }

    #[test]
    fn thread_option_clusters_split_trailing_flags() {
        let normalized = normalize_mixed_noise_option_clusters(
            ["par2", "verify", "-T12qv", "-t1Np", "testfile.par2"]
                .into_iter()
                .map(OsString::from),
        );
        let as_text: Vec<_> = normalized
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            as_text,
            vec![
                "par2",
                "verify",
                "-T12",
                "-q",
                "-t1",
                "-Np",
                "testfile.par2"
            ]
        );
    }
}
