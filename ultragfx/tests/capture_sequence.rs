#![cfg(feature = "capture")]

use ultragfx::capture::{Fixture, Sequence};

fn sequence() -> Sequence {
    let mut frame = Fixture::from_bytes(include_bytes!("fixtures/host64-fill.ugfxcap")).unwrap();
    frame.frame.serial = 1;
    frame.frame.config.clear_policy = ultragfx::ClearPolicy::Persist;
    let mut second = frame.clone();
    second.frame.serial = 2;
    Sequence {
        frames: vec![frame, second],
        warmup_frames: 1,
        presentations: vec![2],
    }
}

#[test]
fn sequence_container_preserves_original_serials_and_versioned_payloads() {
    let sequence = sequence();
    let bytes = sequence.to_bytes().unwrap();
    assert_eq!(Sequence::from_bytes(&bytes).unwrap(), sequence);
    assert!(Fixture::from_bytes(&bytes).is_err());
    for frame in &sequence.frames {
        assert_eq!(&frame.to_bytes().unwrap()[8..12], &3u32.to_le_bytes());
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

#[test]
fn sequence_reads_legacy_v1_frame_payload() {
    let mut frame = include_bytes!("fixtures/host64-fill.ugfxcap").to_vec();
    frame[32..40].copy_from_slice(&1u64.to_le_bytes());
    let mut bytes = b"UGFXCAP\0".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(0x04030201u32.to_le_bytes());
    bytes.extend((56 + frame.len() as u64).to_le_bytes());
    for value in [1u32, 0, 1, 0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(1u64.to_le_bytes());
    bytes.extend((frame.len() as u64).to_le_bytes());
    bytes.extend(frame.clone());
    let sequence = Sequence::from_bytes(&bytes).unwrap();
    assert_eq!(sequence.frames, vec![Fixture::from_bytes(&frame).unwrap()]);
    assert!(Fixture::from_bytes(&bytes).is_err());
    let encoded = sequence.to_bytes().unwrap();
    assert!(Fixture::from_bytes(&encoded).is_err());
    assert_eq!(Sequence::from_bytes(&encoded).unwrap(), sequence);
}
