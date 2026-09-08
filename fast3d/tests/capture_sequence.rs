#![cfg(feature = "capture")]

use fast3d::capture::{Fixture, Sequence};

fn sequence() -> Sequence {
    let mut frame = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.f3dcap")).unwrap();
    frame.frame.serial = 1;
    frame.frame.config.clear_policy = fast3d::ClearPolicy::Persist;
    let mut second = frame.clone();
    second.frame.serial = 2;
    Sequence {
        frames: vec![frame, second],
        warmup_frames: 1,
        presentations: vec![2],
    }
}

#[test]
fn sequence_container_preserves_original_serials_and_v1_payloads() {
    let sequence = sequence();
    let bytes = sequence.to_bytes().unwrap();
    assert_eq!(Sequence::from_bytes(&bytes).unwrap(), sequence);
    assert!(Fixture::from_bytes(&bytes).is_err());
    for frame in &sequence.frames {
        assert_eq!(
            Fixture::from_bytes(&frame.to_bytes().unwrap()).unwrap(),
            *frame
        );
    }
}

#[test]
fn sequence_rejects_cropped_missing_or_reordered_prefix() {
    for serials in [vec![2], vec![1, 3], vec![2, 1], vec![1, 1]] {
        let mut sequence = sequence();
        sequence.frames.truncate(serials.len());
        for (frame, serial) in sequence.frames.iter_mut().zip(serials) {
            frame.frame.serial = serial;
        }
        assert!(sequence.validate().is_err());
    }
}

#[test]
fn sequence_rejects_changed_seed_and_invalid_selection() {
    let mut changed = sequence();
    changed.frames[1].frame.dither_seed ^= 1;
    assert!(changed.validate().is_err());
    for presentations in [vec![], vec![1], vec![3], vec![2, 2]] {
        let mut sequence = sequence();
        sequence.presentations = presentations;
        assert!(sequence.validate().is_err());
    }
}

#[test]
fn sequence_decoder_rejects_truncation_reserved_fields_and_trailing_bytes() {
    let bytes = sequence().to_bytes().unwrap();
    for end in 0..bytes.len() {
        assert!(Sequence::from_bytes(&bytes[..end]).is_err(), "length {end}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Sequence::from_bytes(&trailing).is_err());
    let mut reserved = bytes;
    reserved[36] = 1;
    assert!(Sequence::from_bytes(&reserved).is_err());
}
