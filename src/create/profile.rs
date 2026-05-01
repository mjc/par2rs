use std::time::{Duration, Instant};

const PROFILE_ENV: &str = "PAR2RS_CREATE_PROFILE";

#[derive(Debug, Clone, Copy)]
pub(crate) enum CreateProfilePhase {
    SourceScanMetadata,
    SourceOpenHashPrepass,
    RecoveryChunkProcessing,
    CriticalPacketSerialization,
    RecoveryPacketSerialization,
    OutputFileWrites,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CreateProfileCounters {
    pub source_file_count: usize,
    pub source_bytes: u64,
    pub block_size: u64,
    pub source_block_count: u32,
    pub recovery_block_count: u32,
    pub chunk_size: usize,
    pub defer_hash_computation: bool,
    pub source_hash_bytes_read: u64,
    pub source_recovery_bytes_read: u64,
    pub source_seek_count: u64,
    pub recovery_chunk_count: u64,
    pub selected_backend: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CreateProfile {
    started_at: Instant,
    source_scan_metadata: Duration,
    source_open_hash_prepass: Duration,
    recovery_chunk_processing: Duration,
    critical_packet_serialization: Duration,
    recovery_packet_serialization: Duration,
    output_file_writes: Duration,
    counters: CreateProfileCounters,
}

impl CreateProfile {
    pub(crate) fn from_env() -> Option<Self> {
        let enabled = std::env::var_os(PROFILE_ENV).is_some_and(|value| {
            let value = value.to_string_lossy();
            !matches!(value.as_ref(), "" | "0" | "false" | "FALSE" | "off" | "OFF")
        });
        enabled.then(Self::new)
    }

    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            source_scan_metadata: Duration::ZERO,
            source_open_hash_prepass: Duration::ZERO,
            recovery_chunk_processing: Duration::ZERO,
            critical_packet_serialization: Duration::ZERO,
            recovery_packet_serialization: Duration::ZERO,
            output_file_writes: Duration::ZERO,
            counters: CreateProfileCounters::default(),
        }
    }

    pub(crate) fn add_duration(&mut self, phase: CreateProfilePhase, duration: Duration) {
        match phase {
            CreateProfilePhase::SourceScanMetadata => self.source_scan_metadata += duration,
            CreateProfilePhase::SourceOpenHashPrepass => self.source_open_hash_prepass += duration,
            CreateProfilePhase::RecoveryChunkProcessing => {
                self.recovery_chunk_processing += duration;
            }
            CreateProfilePhase::CriticalPacketSerialization => {
                self.critical_packet_serialization += duration;
            }
            CreateProfilePhase::RecoveryPacketSerialization => {
                self.recovery_packet_serialization += duration;
            }
            CreateProfilePhase::OutputFileWrites => self.output_file_writes += duration,
        }
    }

    pub(crate) fn counters_mut(&mut self) -> &mut CreateProfileCounters {
        &mut self.counters
    }

    #[cfg(test)]
    pub(crate) fn counters(&self) -> &CreateProfileCounters {
        &self.counters
    }

    pub(crate) fn emit(&self) {
        eprintln!("PAR2RS_CREATE_PROFILE_BEGIN");
        eprintln!("phase,seconds");
        eprintln!(
            "source_scan_metadata,{:.9}",
            self.source_scan_metadata.as_secs_f64()
        );
        eprintln!(
            "source_open_hash_prepass,{:.9}",
            self.source_open_hash_prepass.as_secs_f64()
        );
        eprintln!(
            "recovery_chunk_processing,{:.9}",
            self.recovery_chunk_processing.as_secs_f64()
        );
        eprintln!(
            "critical_packet_serialization,{:.9}",
            self.critical_packet_serialization.as_secs_f64()
        );
        eprintln!(
            "recovery_packet_serialization,{:.9}",
            self.recovery_packet_serialization.as_secs_f64()
        );
        eprintln!(
            "output_file_writes,{:.9}",
            self.output_file_writes.as_secs_f64()
        );
        eprintln!(
            "total_create_process,{:.9}",
            self.started_at.elapsed().as_secs_f64()
        );
        eprintln!("counter,value");
        eprintln!("source_file_count,{}", self.counters.source_file_count);
        eprintln!("source_bytes,{}", self.counters.source_bytes);
        eprintln!("block_size,{}", self.counters.block_size);
        eprintln!("source_block_count,{}", self.counters.source_block_count);
        eprintln!(
            "recovery_block_count,{}",
            self.counters.recovery_block_count
        );
        eprintln!("chunk_size,{}", self.counters.chunk_size);
        eprintln!(
            "defer_hash_computation,{}",
            self.counters.defer_hash_computation
        );
        eprintln!(
            "source_hash_bytes_read,{}",
            self.counters.source_hash_bytes_read
        );
        eprintln!(
            "source_recovery_bytes_read,{}",
            self.counters.source_recovery_bytes_read
        );
        eprintln!("source_seek_count,{}", self.counters.source_seek_count);
        eprintln!(
            "recovery_chunk_count,{}",
            self.counters.recovery_chunk_count
        );
        eprintln!(
            "selected_create_backend,{}",
            self.counters
                .selected_backend
                .as_deref()
                .unwrap_or("unknown")
        );
        eprintln!("PAR2RS_CREATE_PROFILE_END");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn from_env_respects_falsey_and_truthy_values() {
        let _guard = env_lock();
        std::env::remove_var(PROFILE_ENV);
        assert!(CreateProfile::from_env().is_none());

        for value in ["", "0", "false", "FALSE", "off", "OFF"] {
            std::env::set_var(PROFILE_ENV, value);
            assert!(
                CreateProfile::from_env().is_none(),
                "value {value} should disable profiling"
            );
        }

        for value in ["1", "true", "on", "yes"] {
            std::env::set_var(PROFILE_ENV, value);
            assert!(
                CreateProfile::from_env().is_some(),
                "value {value} should enable profiling"
            );
        }

        std::env::remove_var(PROFILE_ENV);
    }

    #[test]
    fn profile_accumulates_phase_durations_and_counters() {
        let mut profile = CreateProfile::new();

        profile.add_duration(
            CreateProfilePhase::SourceScanMetadata,
            Duration::from_millis(5),
        );
        profile.add_duration(
            CreateProfilePhase::SourceOpenHashPrepass,
            Duration::from_millis(7),
        );
        profile.add_duration(
            CreateProfilePhase::RecoveryChunkProcessing,
            Duration::from_millis(11),
        );
        profile.add_duration(
            CreateProfilePhase::CriticalPacketSerialization,
            Duration::from_millis(13),
        );
        profile.add_duration(
            CreateProfilePhase::RecoveryPacketSerialization,
            Duration::from_millis(17),
        );
        profile.add_duration(
            CreateProfilePhase::OutputFileWrites,
            Duration::from_millis(19),
        );

        let counters = profile.counters_mut();
        counters.source_file_count = 2;
        counters.source_bytes = 123;
        counters.block_size = 4;
        counters.source_block_count = 6;
        counters.recovery_block_count = 1;
        counters.chunk_size = 4;
        counters.defer_hash_computation = true;
        counters.source_hash_bytes_read = 100;
        counters.source_recovery_bytes_read = 80;
        counters.source_seek_count = 3;
        counters.recovery_chunk_count = 2;
        counters.selected_backend = Some("scalar".to_string());

        assert_eq!(profile.source_scan_metadata, Duration::from_millis(5));
        assert_eq!(profile.source_open_hash_prepass, Duration::from_millis(7));
        assert_eq!(profile.recovery_chunk_processing, Duration::from_millis(11));
        assert_eq!(
            profile.critical_packet_serialization,
            Duration::from_millis(13)
        );
        assert_eq!(
            profile.recovery_packet_serialization,
            Duration::from_millis(17)
        );
        assert_eq!(profile.output_file_writes, Duration::from_millis(19));
        assert_eq!(profile.counters.source_file_count, 2);
        assert_eq!(profile.counters.selected_backend.as_deref(), Some("scalar"));

        profile.emit();
    }
}
