//! Browser OSC 3: OSC 1's shared generator/controller and OSC 2's tuning law.
use super::primary;
use radias_synth_application::primary::ThirdProgram;
use radias_synth_domain::{
    Sample,
    fixed::{multiply_q15, saturate},
    modulation::AppliedModulationTargets,
    pitch::{PhaseIncrement, PitchCode},
    primary_oscillator::PrimaryOscillator,
    secondary_control::SecondaryModulation,
    voice::VoiceParameters,
    waveform::WaveformTable,
};

pub(super) fn interaction(
    program: ThirdProgram,
    targets: AppliedModulationTargets,
) -> (SecondaryModulation, usize) {
    let mode = (program.interaction as i32 + targets.controls[48] as i32).clamp(0, 3) as u8;
    let reference = (program.reference as i32 + targets.controls[49] as i32).clamp(0, 2) as usize;
    (
        SecondaryModulation {
            ring: mode & 1 != 0,
            sync: mode & 2 != 0,
        },
        reference,
    )
}

pub(super) struct Pitch {
    base: u32,
    offset: u64,
    source_code: u16,
    increment: PhaseIncrement,
    code: PitchCode,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn next(
    oscillator: &mut PrimaryOscillator,
    controller: &mut primary::Controller,
    cached: &mut Option<Pitch>,
    mut program: ThirdProgram,
    targets: AppliedModulationTargets,
    p: VoiceParameters,
    cv: [f64; 5],
    modulator: Sample,
    reference: Sample,
    reference_wrap: bool,
    tables: &primary::Tables,
    fine: &radias_synth_domain::controller_secondary::FineTuneTable,
    table: &WaveformTable,
    frame: u64,
    id: usize,
) -> Sample {
    let wave =
        ((program.primary.selection & 15) as i32 + targets.controls[44] as i32).clamp(0, 5) as u8;
    let mode = if wave >= 4 {
        0
    } else {
        ((program.primary.selection >> 4) as i32 + targets.controls[45] as i32).clamp(0, 3) as u8
    };
    program.primary.selection = wave | (mode << 4);
    program.pitch.semitone =
        (program.pitch.semitone as i32 + targets.controls[46] as i32).clamp(0, 127) as u8;
    program.pitch.fine_tune =
        (program.pitch.fine_tune as i32 + targets.controls[47] as i32).clamp(0, 127) as u8;
    program.pitch.virtual_patch_q16 = targets.oscillator_pitch_q16[2];
    let relative = program.pitch.relative_code(fine) as f64 + cv[3] * 24.0 * 256.0;
    let base = p.primary.base_increment().0;
    if cached.as_ref().is_none_or(|c| {
        c.base != base || c.offset != relative.to_bits() || c.source_code != p.primary_pitch_code
    }) {
        *cached = Some(Pitch {
            base,
            offset: relative.to_bits(),
            source_code: p.primary_pitch_code,
            increment: PhaseIncrement(
                (base as f64 * 2.0_f64.powf(relative / (12.0 * 256.0))).clamp(0.0, i32::MAX as f64)
                    as u32,
            ),
            code: PitchCode::new(
                (p.primary_pitch_code as f64 + relative)
                    .round()
                    .clamp(0.0, 32767.0) as u16,
            )
            .unwrap(),
        });
    }
    let pitch = cached.as_ref().unwrap();
    let (interaction, _) = interaction(program, targets);
    let sample = controller.next_with_sync(
        oscillator,
        program.primary,
        pitch.increment,
        pitch.code,
        ((cv[4] * 32768.0).round().clamp(-32768.0, 32767.0) as i16) >> 8,
        [
            (targets.controls[42] as f64 + cv[1] * 16383.0)
                .round()
                .clamp(-32767.0, 32767.0) as i16,
            (targets.controls[43] as f64 + cv[2] * 127.0)
                .round()
                .clamp(-127.0, 127.0) as i16,
        ],
        modulator,
        tables,
        table,
        frame,
        id,
        interaction.sync && reference_wrap,
    );
    if interaction.ring {
        Sample(saturate(multiply_q15(sample.0, (reference.0 >> 16) as i16)))
    } else {
        sample
    }
}
