//! Animation processing for ASS effects

/// Calculate progress for \move animation
pub fn calculate_move_progress(time_cs: u32, t1: u32, t2: u32) -> f32 {
    calculate_progress_ms(
        u64::from(time_cs) * 10,
        u64::from(t1) * 10,
        u64::from(t2) * 10,
    )
}

/// Calculate progress for \fade animation
pub fn calculate_fade_progress(time_cs: u32, t1: u32, t2: u32) -> f32 {
    calculate_progress_ms(
        u64::from(time_cs) * 10,
        u64::from(t1) * 10,
        u64::from(t2) * 10,
    )
}

/// Millisecond-native progress for `\move`/`\fade`/`\t` interpolation.
///
/// `time_ms` and the bounds share the renderer's ms clock, so sub-centisecond
/// times interpolate smoothly instead of stepping every 10ms.
pub fn calculate_progress_ms(time_ms: u64, t1_ms: u64, t2_ms: u64) -> f32 {
    if time_ms <= t1_ms {
        0.0
    } else if time_ms >= t2_ms {
        1.0
    } else {
        (time_ms - t1_ms) as f32 / (t2_ms - t1_ms) as f32
    }
}
