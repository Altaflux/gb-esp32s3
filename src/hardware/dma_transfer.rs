use esp_hal::{
    dma::DmaTxBuf,
    spi::{
        master::{InstanceDma, SpiDma, SpiDmaTransfer},
        FullDuplexMode,
    },
    Mode,
};

use super::LineTransfer;

enum DmaState<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    IDLE(SpiDma<'d, T, FullDuplexMode, M>),
    RUNNING(SpiDmaTransfer<'d, T, FullDuplexMode, M, DmaTxBuf>),
}

pub struct DmaTransfer<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    dma: Option<DmaState<'d, T, M>>,
    spare_buffer: Option<DmaTxBuf>,
}

impl<'d, T, M> DmaTransfer<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    pub fn new(spi: SpiDma<'d, T, FullDuplexMode, M>, buffer: DmaTxBuf) -> Self {
        Self {
            spare_buffer: Some(buffer),
            dma: Some(DmaState::IDLE(spi)),
        }
    }
    pub fn do_transfer(&mut self, buffer: DmaTxBuf) -> DmaTxBuf {
        let dma_state: DmaState<'d, T, M> = core::mem::replace(&mut self.dma, None).unwrap();
        let (spi, spare_buffer) = match dma_state {
            DmaState::IDLE(spi_dma) => {
                let second_buffer = core::mem::replace(&mut self.spare_buffer, None).unwrap();
                (spi_dma, second_buffer)
            }
            DmaState::RUNNING(spi_dma_transfer) => spi_dma_transfer.wait(),
        };
        self.dma = Some(DmaState::RUNNING(spi.dma_write(buffer).unwrap()));
        spare_buffer
    }

    pub fn free(mut self) -> (SpiDma<'d, T, FullDuplexMode, M>, DmaTxBuf) {
        let dma_state = core::mem::replace(&mut self.dma, None).unwrap();
        let (spi, spare_buffer) = match dma_state {
            DmaState::IDLE(spi_dma) => {
                let second_buffer = core::mem::replace(&mut self.spare_buffer, None).unwrap();
                (spi_dma, second_buffer)
            }
            DmaState::RUNNING(spi_dma_transfer) => spi_dma_transfer.wait(),
        };

        (spi, spare_buffer)
    }
}

impl<'d, T, M> LineTransfer for DmaTransfer<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    fn send_scanline(&mut self, line: DmaTxBuf) -> DmaTxBuf {
        self.do_transfer(line)
    }
}
