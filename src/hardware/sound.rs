use gb_core::hardware::sound::AudioPlayer;

pub struct NullAudioPlayer;

impl AudioPlayer for NullAudioPlayer {
    fn play(&mut self, _output_buffer: &[u16]) {}

    fn samples_rate(&self) -> u32 {
        16000
    }

    fn underflowed(&self) -> bool {
        false
    }
}
