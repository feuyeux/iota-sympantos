use super::*;

#[test]
fn warm_count_only_increments_for_newly_started_backend() {
    let mut warmed = 0;

    record_warm_result(&mut warmed, false);
    record_warm_result(&mut warmed, true);

    assert_eq!(warmed, 1);
}
