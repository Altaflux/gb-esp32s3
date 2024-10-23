use esp_hal::dma::DmaTxBuf;

pub mod dma_streamer;
pub mod dma_transfer;
pub mod scaler;
trait LineTransfer {
    fn send_scanline(&mut self, line: DmaTxBuf) -> DmaTxBuf;
}
