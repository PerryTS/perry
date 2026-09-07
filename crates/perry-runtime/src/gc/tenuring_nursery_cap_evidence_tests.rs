use super::super::policy::ScavengeNurseryCapTestGuard;
use super::*;

type FixtureARow = (usize, usize, usize, usize, usize, bool);

/// TN5 unset, 25 copying minors. Columns are `(from_space_bytes,
/// nursery_cap_bytes, survival_permille, copied_bytes,
/// mean_surviving_object_bytes_after, cap_due)`.
const FIXTURE_A: [FixtureARow; 25] = [
    (15_728_712, 15_367_929, 839, 13_206_480, 77, true),
    (17_407_296, 16_777_216, 936, 3_214_304, 84, true),
    (33_624_624, 33_554_432, 757, 22_240_712, 133, true),
    (34_829_760, 33_554_432, 396, 5_672_776, 84, true),
    (39_313_552, 67_108_864, 506, 14_979_552, 118, false),
    (67_407_992, 67_108_864, 261, 3_267_840, 110, true),
    (67_230_456, 67_108_864, 60, 1_181_376, 61, true),
    (57_803_984, 56_841_207, 30, 1_182_328, 40, true),
    (37_882_104, 37_245_419, 39, 725_224, 47, true),
    (44_762_032, 43_754_979, 28, 769_240, 43, true),
    (40_614_704, 40_063_991, 16, 649_840, 36, true),
    (34_204_000, 33_554_432, 26, 921_496, 38, true),
    (35_524_488, 35_366_371, 72, 2_558_256, 80, true),
    (67_572_776, 67_108_864, 56, 1_517_184, 63, true),
    (59_188_240, 58_720_256, 33, 970_776, 45, true),
    (42_913_584, 41_943_040, 30, 1_330_288, 39, true),
    (36_981_552, 36_305_895, 86, 3_185_688, 84, true),
    (56_662_480, 67_108_864, 68, 815_448, 68, false),
    (63_730_280, 63_350_767, 22, 1_029_672, 37, true),
    (34_583_744, 34_426_847, 32, 1_119_864, 39, true),
    (36_427_200, 36_305_895, 19, 637_536, 40, true),
    (37_347_336, 37_245_419, 90, 3_395_304, 73, true),
    (67_358_392, 67_108_864, 60, 1_053_088, 62, true),
    (58_724_472, 57_780_731, 24, 893_864, 38, true),
    (35_510_472, 35_366_371, 31, 1_118_240, 39, true),
];

type FixtureBRow = (usize, usize, usize);

/// NS3 n16, 26 copying minors. Columns are `(from_space_bytes,
/// nursery_cap_bytes, eden_live_bytes)`. The four transition rows retain the
/// exact diagnostic value; other rows use `survival_permille * from_space /
/// 1000`, as specified by the fixture contract.
const FIXTURE_B: [FixtureBRow; 26] = [
    (15_733_256, 15_367_929, 13_200_201),
    (17_401_448, 16_777_216, 3_208_856),
    (34_602_744, 33_554_432, 25_294_605),
    (35_460_656, 33_554_432, 9_684_456),
    (68_156_728, 67_108_864, 6_815_672),
    (67_837_240, 67_108_864, 542_697),
    (59_767_832, 59_592_671, 46_176),
    (34_602_608, 33_554_432, 34_602),
    (31_499_296, 33_554_432, 2_803_437),
    (34_086_960, 33_554_432, 2_590_608),
    (34_599_896, 33_554_432, 32_928),
    (20_971_160, 20_287_508, 41_942),
    (20_955_528, 20_479_960, 125_733),
    (22_158_808, 21_317_124, 3_213_027),
    (22_002_944, 21_367_748, 3_102_415),
    (23_068_384, 22_920_104, 23_068),
    (23_068_336, 22_938_412, 23_068),
    (23_068_344, 22_956_972, 23_068),
    (24_160_512, 23_127_680, 459_049),
    (19_350_992, 19_195_148, 3_057_456),
    (19_846_320, 19_196_388, 3_155_564),
    (20_971_416, 20_775_896, 20_971),
    (20_971_392, 20_793_888, 20_971),
    (21_006_848, 20_793_888, 21_006),
    (22_047_208, 21_018_128, 44_094),
    (21_023_432, 21_018_128, 315_351),
];

fn cap_scale() -> u8 {
    NURSERY_CAP_SCALE.with(Cell::get)
}

fn cap_grow_streak() -> u8 {
    CAP_GROW_STREAK.with(Cell::get)
}

fn seed_allocation_mean(mean: usize) {
    assert!(!COPYING_MINOR_COMPLETED.with(Cell::get));
    MEAN_SURVIVING_OBJECT_BYTES.with(|cell| cell.set(mean));
    OBJECT_CENSUS_SEEDED.with(|cell| cell.set(true));
}

fn replay_fixture_b() -> Vec<u8> {
    let mut scales = Vec::with_capacity(FIXTURE_B.len());
    for &(from_space, cap, eden_live) in &FIXTURE_B {
        note_surviving_object_census(NURSERY_CAP_REFERENCE_OBJECT_BYTES, 1);
        super::retune_after_scavenge(
            eden_live,
            0,
            0,
            NurseryCapRating::from_values(from_space, cap),
        );
        scales.push(cap_scale());
    }
    scales
}

/// Sabotage: let the survivor census feed the steady cap again; the expected
/// 64 MiB suffix reproduces TN5's 32--42 MiB dips instead.
#[test]
fn steady_state_cap_is_not_object_denominated_after_first_copying_minor() {
    reset_for_test();
    let _cap_guard = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let base = gc_scavenge_nursery_cap_bytes();
    seed_allocation_mean(66);

    let mut effective_caps = Vec::with_capacity(FIXTURE_A.len());
    for &(from_space, recorded_cap, survival_permille, copied_bytes, mean_after, cap_due) in
        &FIXTURE_A
    {
        assert!(survival_permille <= 1000);
        assert_eq!(cap_due, from_space >= recorded_cap);
        effective_caps.push(influx_driven_nursery_cap_bytes());

        note_surviving_object_census(mean_after, 1);
        super::retune_after_scavenge(
            copied_bytes,
            0,
            0,
            NurseryCapRating::from_values(from_space, recorded_cap),
        );
    }

    assert_eq!(effective_caps[0], 15_367_929);
    assert_eq!(&effective_caps[1..4], &[base, base * 2, base * 2]);
    for (row, &effective_cap) in FIXTURE_A.iter().zip(&effective_caps).skip(4) {
        if row.5 {
            assert_eq!(effective_cap, base * NURSERY_CAP_SCALE_MAX as usize);
        }
    }
    assert_eq!(cap_scale(), NURSERY_CAP_SCALE_MAX);
    reset_for_test();
}

/// Sabotage: replace the first-minor regime with the steady byte band; the
/// 40 B mean receives the raw 1000-per-mille band instead of today's 555.
#[test]
fn first_minor_keeps_the_object_denomination() {
    reset_for_test();
    let _cap_guard = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    seed_allocation_mean(40);

    // 40 * 1000 / 72 = 555. The unchanged 500-per-mille floor begins below
    // 36 B; claiming 500 here would silently re-tune the preserved regime.
    assert_eq!(nursery_cap_object_scale_permille(40), 555);
    assert_eq!(influx_driven_nursery_cap_bytes(), 9_311_354);
    assert!(!COPYING_MINOR_COMPLETED.with(Cell::get));
    reset_for_test();
}

/// Sabotage: restore the `< cap / 100` mortality branch; the low-survival
/// suffix lowers the scale instead of leaving it at the ceiling.
#[test]
fn nursery_cap_scale_does_not_shrink_on_low_mortality() {
    reset_for_test();
    let _cap_guard = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let scales = replay_fixture_b();

    assert_eq!(scales[3], NURSERY_CAP_SCALE_MAX);
    assert!(scales[3..].iter().all(|&scale| scale == 4));
    assert_eq!(scales[25], 4);
    reset_for_test();
}

/// Sabotage: remove or re-time the influx grow branch; this exact transition
/// vector changes at row 2 or row 4.
#[test]
fn nursery_cap_scale_still_grows_on_influx() {
    reset_for_test();
    let _cap_guard = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let scales = replay_fixture_b();
    let mut expected = vec![4; FIXTURE_B.len()];
    expected[0] = 1;
    expected[1] = 2;
    expected[2] = 2;

    assert_eq!(scales, expected, "growth must occur only at rows 2 and 4");
    reset_for_test();
}

/// Sabotage: delete the cap-due return; two high-influx forced minors advance
/// the streak and grow the scale to 2.
#[test]
fn non_cap_due_minor_does_not_rate_the_scale() {
    reset_for_test();
    let _cap_guard = ScavengeNurseryCapTestGuard::due_at_bytes(1);
    note_surviving_object_census(NURSERY_CAP_REFERENCE_OBJECT_BYTES, 1);
    let base = gc_scavenge_nursery_cap_bytes();
    let not_due = NurseryCapRating::from_values(base - 1, base);

    super::retune_after_scavenge(base, 0, 0, not_due);
    super::retune_after_scavenge(base, 0, 0, not_due);

    assert_eq!(cap_scale(), 1);
    assert_eq!(cap_grow_streak(), 0);
    reset_for_test();
}
