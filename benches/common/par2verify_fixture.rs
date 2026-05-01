#![allow(dead_code)]

use par2rs::create::{CreateContextBuilder, SilentCreateReporter};
use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DEFAULT_TURBO_ROOT: &str = "/home/mjc/projects/par2cmdline-turbo";
const DATA_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct VerifyFixture {
    pub case_name: &'static str,
    pub dir: PathBuf,
    pub par2_file: PathBuf,
    pub protected_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct TurboCommand {
    program: PathBuf,
    use_verify_subcommand: bool,
}

static CRITERION_FIXTURE: OnceLock<VerifyFixture> = OnceLock::new();
static IAI_FIXTURE: OnceLock<VerifyFixture> = OnceLock::new();
static PAR2RS_VERIFY: OnceLock<PathBuf> = OnceLock::new();
static TURBO_VERIFY: OnceLock<TurboCommand> = OnceLock::new();

pub fn criterion_fixture() -> &'static VerifyFixture {
    CRITERION_FIXTURE
        .get_or_init(|| create_fixture("criterion-intact-64m", 64 * 1024 * 1024, 1024 * 1024))
}

pub fn iai_fixture() -> &'static VerifyFixture {
    IAI_FIXTURE.get_or_init(|| create_fixture("iai-intact-8m", 8 * 1024 * 1024, 256 * 1024))
}

pub fn iai_fixture_arg() -> String {
    iai_fixture().par2_file.to_string_lossy().into_owned()
}

pub fn par2rs_verify_invocation(fixture: &VerifyFixture) -> Invocation {
    verify_invocation(resolve_par2rs_verify().clone(), false, &fixture.par2_file)
}

pub fn turbo_verify_invocation(fixture: &VerifyFixture) -> Invocation {
    let turbo = resolve_turbo_verify().clone();
    verify_invocation(
        turbo.program,
        turbo.use_verify_subcommand,
        &fixture.par2_file,
    )
}

pub fn par2rs_verify_invocation_for_path(par2_file: impl AsRef<Path>) -> Invocation {
    verify_invocation(resolve_par2rs_verify().clone(), false, par2_file.as_ref())
}

pub fn turbo_verify_invocation_for_path(par2_file: impl AsRef<Path>) -> Invocation {
    let turbo = resolve_turbo_verify().clone();
    verify_invocation(
        turbo.program,
        turbo.use_verify_subcommand,
        par2_file.as_ref(),
    )
}

fn verify_invocation(
    program: PathBuf,
    use_verify_subcommand: bool,
    par2_file: &Path,
) -> Invocation {
    let mut args = Vec::with_capacity(3);
    if use_verify_subcommand {
        args.push(OsString::from("verify"));
    }
    args.push(OsString::from("-q"));
    args.push(par2_file.as_os_str().to_os_string());

    let current_dir = par2_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir());

    Invocation {
        program,
        args,
        current_dir,
    }
}

fn create_fixture(
    case_name: &'static str,
    protected_bytes: usize,
    block_size: u64,
) -> VerifyFixture {
    let dir = target_dir()
        .join("bench-fixtures")
        .join("par2verify")
        .join(case_name);
    let source_file = dir.join("payload.bin");
    let par2_file = dir.join("payload.par2");

    if !source_file.is_file() || !par2_file.is_file() {
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("failed to clear stale benchmark fixture directory");
        }
        fs::create_dir_all(&dir).expect("failed to create benchmark fixture directory");
        write_payload_file(&source_file, protected_bytes)
            .expect("failed to create benchmark payload");

        let reporter = Box::new(SilentCreateReporter);
        let mut context = CreateContextBuilder::new()
            .output_name(par2_file.to_string_lossy().into_owned())
            .source_files(vec![source_file.clone()])
            .block_size(block_size)
            .redundancy_percentage(10)
            .overwrite_existing(true)
            .reporter(reporter)
            .build()
            .expect("failed to build benchmark create context");

        context
            .create()
            .expect("failed to create benchmark PAR2 fixture");
    }

    VerifyFixture {
        case_name,
        dir,
        par2_file,
        protected_bytes: protected_bytes as u64,
    }
}

fn write_payload_file(path: &Path, total_bytes: usize) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut state = 0x1234_5678u32;
    let mut chunk = vec![0u8; DATA_CHUNK_BYTES];
    let mut remaining = total_bytes;

    while remaining > 0 {
        let take = remaining.min(chunk.len());
        for byte in &mut chunk[..take] {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (state >> 16) as u8;
        }
        writer.write_all(&chunk[..take])?;
        remaining -= take;
    }

    writer.flush()
}

fn resolve_par2rs_verify() -> &'static PathBuf {
    PAR2RS_VERIFY.get_or_init(|| {
        if let Some(path) = option_env!("CARGO_BIN_EXE_par2verify") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return path;
            }
        }

        for candidate in [
            target_dir().join("release").join("par2verify"),
            target_dir().join("debug").join("par2verify"),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }

        panic!(
            "could not resolve par2rs par2verify binary; run cargo build --release --bin par2verify or cargo bench from a Cargo-built environment"
        );
    })
}

fn resolve_turbo_verify() -> &'static TurboCommand {
    TURBO_VERIFY.get_or_init(|| {
        let turbo_root = env::var_os("TURBO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_TURBO_ROOT));

        if let Some(path) = env::var_os("TURBO_PAR2VERIFY")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            return TurboCommand {
                program: path,
                use_verify_subcommand: false,
            };
        }

        if let Some(path) = find_in_path("par2verify") {
            return TurboCommand {
                program: path,
                use_verify_subcommand: false,
            };
        }

        if let Some(path) = env::var_os("TURBO_PAR2")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            let wrapper = path
                .parent()
                .map(|dir| dir.join("par2verify"))
                .filter(|wrapper| wrapper.is_file());
            if let Some(wrapper) = wrapper {
                return TurboCommand {
                    program: wrapper,
                    use_verify_subcommand: false,
                };
            }
            return TurboCommand {
                program: path,
                use_verify_subcommand: true,
            };
        }

        let root_wrapper = turbo_root.join("par2verify");
        if root_wrapper.is_file() {
            return TurboCommand {
                program: root_wrapper,
                use_verify_subcommand: false,
            };
        }

        let root_par2 = turbo_root.join("par2");
        if root_par2.is_file() {
            return TurboCommand {
                program: root_par2,
                use_verify_subcommand: true,
            };
        }

        if let Some(path) = find_in_path("par2") {
            return TurboCommand {
                program: path,
                use_verify_subcommand: true,
            };
        }

        panic!(
            "could not resolve par2cmdline-turbo verify command; run inside nix develop or set TURBO_PAR2VERIFY/TURBO_PAR2"
        );
    })
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn target_dir() -> PathBuf {
    match env::var_os("CARGO_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_absolute() {
                path
            } else {
                manifest_dir().join(path)
            }
        }
        None => manifest_dir().join("target"),
    }
}
