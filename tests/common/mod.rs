use std::path::{Path, PathBuf};

pub const PAR1_FLATDATA_FILES: [(&str, &[(usize, u8)]); 10] = [
    (
        "test-0.data",
        &[
            (18_593, 1),
            (11_835, 2),
            (10_742, 3),
            (15_039, 4),
            (9_681, 5),
        ],
    ),
    (
        "test-1.data",
        &[
            (8_834, 5),
            (10_703, 6),
            (10_664, 7),
            (18_085, 8),
            (13_203, 9),
            (17_695, 10),
            (19_023, 11),
            (17_421, 12),
            (14_687, 13),
            (17_226, 14),
            (10_820, 15),
            (13_437, 16),
            (5_376, 17),
        ],
    ),
    (
        "test-2.data",
        &[
            (9_506, 17),
            (14_414, 18),
            (18_750, 19),
            (13_750, 20),
            (14_179, 21),
            (18_476, 22),
            (546, 23),
        ],
    ),
    (
        "test-3.data",
        &[
            (12_735, 23),
            (12_500, 24),
            (13_125, 25),
            (18_437, 26),
            (15_390, 27),
            (12_617, 28),
            (16_171, 29),
            (11_562, 30),
            (11_523, 31),
            (10_156, 32),
            (7_913, 33),
        ],
    ),
    (
        "test-4.data",
        &[
            (10_290, 33),
            (13_984, 34),
            (11_445, 35),
            (11_523, 36),
            (13_281, 37),
            (13_945, 38),
            (18_359, 39),
            (9_298, 40),
        ],
    ),
    (
        "test-5.data",
        &[
            (3_436, 40),
            (16_171, 41),
            (17_812, 42),
            (11_445, 43),
            (11_796, 44),
            (16_289, 45),
            (18_125, 46),
            (4_876, 47),
        ],
    ),
    (
        "test-6.data",
        &[
            (6_374, 47),
            (12_968, 48),
            (13_906, 49),
            (14_453, 50),
            (16_992, 51),
            (13_828, 52),
            (19_335, 53),
            (16_757, 54),
            (14_787, 55),
        ],
    ),
    (
        "test-7.data",
        &[
            (2_322, 55),
            (14_921, 56),
            (14_023, 57),
            (11_015, 58),
            (11_679, 59),
            (11_757, 60),
            (2_018, 61),
        ],
    ),
    (
        "test-8.data",
        &[
            (12_747, 61),
            (17_695, 62),
            (17_500, 63),
            (19_218, 64),
            (3_447, 65),
        ],
    ),
    (
        "test-9.data",
        &[
            (16_474, 65),
            (12_304, 66),
            (16_093, 67),
            (18_710, 68),
            (18_281, 69),
            (18_906, 70),
            (3_177, 71),
        ],
    ),
];

pub fn par1_flatdata_file_bytes(name: &str) -> Vec<u8> {
    let (_, runs) = PAR1_FLATDATA_FILES
        .iter()
        .find(|(file_name, _)| *file_name == name)
        .copied()
        .unwrap_or_else(|| panic!("unknown PAR1 flatdata file {name}"));
    runs.iter()
        .flat_map(|(len, byte)| std::iter::repeat_n(*byte, *len))
        .collect()
}

pub fn write_par1_flatdata_files(dir: &Path) {
    PAR1_FLATDATA_FILES.iter().for_each(|(name, _)| {
        std::fs::write(dir.join(name), par1_flatdata_file_bytes(name))
            .unwrap_or_else(|err| panic!("failed to write {name}: {err}"));
    });
}

pub fn copy_par1_flatdata_recovery_files(dir: &Path) {
    let fixture_dir = Path::new("tests/fixtures/par1/flatdata");
    ["testdata.par", "testdata.p01", "testdata.p02"]
        .iter()
        .for_each(|name| {
            std::fs::copy(fixture_dir.join(name), dir.join(name))
                .unwrap_or_else(|err| panic!("failed to copy {name}: {err}"));
        });
}

pub fn prepare_par1_flatdata_fixture(dir: &Path) -> PathBuf {
    copy_par1_flatdata_recovery_files(dir);
    write_par1_flatdata_files(dir);
    dir.join("testdata.par")
}
