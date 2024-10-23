use super::{dma_transfer::DmaTransfer, LineTransfer};
use display_interface::{DataFormat, DisplayError, WriteOnlyDataCommand};
use esp_hal::{
    dma::DmaTxBuf,
    spi::{
        master::{InstanceDma, SpiDma},
        FullDuplexMode,
    },
    Mode,
};

pub struct DmaStreamer<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    spare_buffer: Option<DmaTxBuf>,
    main_buffer: Option<DmaTxBuf>,
    spi: Option<SpiDma<'d, T, FullDuplexMode, M>>,
}

impl<'d, T, M> DmaStreamer<'d, T, M>
where
    T: InstanceDma,
    M: Mode,
{
    pub fn new(
        spi: SpiDma<'d, T, FullDuplexMode, M>,
        main_buffer: DmaTxBuf,
        spare_buffer: DmaTxBuf,
    ) -> Self {
        Self {
            spi: Some(spi),
            spare_buffer: Some(spare_buffer),
            main_buffer: Some(main_buffer),
        }
    }
    pub fn stream_u8(&mut self, iterator: &mut dyn Iterator<Item = u8>) {
        let spare_buffer = core::mem::replace(&mut self.spare_buffer, None).unwrap();
        let main_buffer = core::mem::replace(&mut self.main_buffer, None).unwrap();
        let tx = core::mem::replace(&mut self.spi, None).unwrap();
        let stream = DmaTransfer::new(tx, main_buffer);

        let (stream, spare_buffer) = Self::compute_line_u8(stream, spare_buffer, iterator);

        let (spi, main_buffer) = stream.free();
        self.main_buffer = Some(main_buffer);
        self.spare_buffer = Some(spare_buffer);
        self.spi = Some(spi);
    }

    pub fn stream_u16<F>(&mut self, iterator: &mut dyn Iterator<Item = u16>, f: F)
    where
        F: Fn(u16) -> [u8; 2],
    {
        let spare_buffer = core::mem::replace(&mut self.spare_buffer, None).unwrap();
        let main_buffer = core::mem::replace(&mut self.main_buffer, None).unwrap();

        let tx = core::mem::replace(&mut self.spi, None).unwrap();
        let stream = DmaTransfer::new(tx, main_buffer);
        let (stream, spare_buffer) = Self::compute_line_u16(stream, spare_buffer, iterator, f);

        let (spi, main_buffer) = stream.free();
        self.main_buffer = Some(main_buffer);
        self.spare_buffer = Some(spare_buffer);
        self.spi = Some(spi);
    }

    #[inline(always)]
    fn compute_line_u8<LT: LineTransfer>(
        mut transfer: LT,
        mut buffer: DmaTxBuf,
        iterator: &mut dyn Iterator<Item = u8>,
    ) -> (LT, DmaTxBuf) {
        let mut width_position = 0;
        for pixel in iterator {
            let out = pixel;
            buffer.as_mut_slice()[width_position] = out;
            width_position += 1;
            if width_position == buffer.len() {
                buffer.set_length(width_position);
                buffer = transfer.send_scanline(buffer);
                width_position = 0;
            }
        }

        if width_position > 0 {
            buffer.set_length(width_position);
            buffer = transfer.send_scanline(buffer);
        }

        (transfer, buffer)
    }

    #[inline(always)]
    fn compute_line_u16<LT: LineTransfer, F>(
        mut transfer: LT,
        mut tx_buffer: DmaTxBuf,
        iterator: &mut dyn Iterator<Item = u16>,
        f: F,
    ) -> (LT, DmaTxBuf)
    where
        F: Fn(u16) -> [u8; 2],
    {
        let mut width_position = 0;
        for pixel in iterator.map(f) {
            let [p1, p2] = pixel;
            tx_buffer.as_mut_slice()[(width_position) + 0] = p1;
            tx_buffer.as_mut_slice()[(width_position) + 1] = p2;
            width_position += 2;
            if width_position == tx_buffer.capacity() {
                tx_buffer.set_length(width_position);
                tx_buffer = transfer.send_scanline(tx_buffer);
                width_position = 0;
            }
        }

        if width_position > 0 {
            tx_buffer.set_length(width_position);
            tx_buffer = transfer.send_scanline(tx_buffer);
        }
        (transfer, tx_buffer)
    }
}

use embedded_hal::digital::OutputPin;
type Result = core::result::Result<(), DisplayError>;
pub struct SpiDmaCMInterface<'d, T, M, RS>
where
    T: InstanceDma,
    M: Mode,
{
    streamer: DmaStreamer<'d, T, M>,
    rs: RS,
}

impl<'d, T, M, RS> SpiDmaCMInterface<'d, T, M, RS>
where
    T: InstanceDma,
    M: Mode,
{
    pub fn new(streamer: DmaStreamer<'d, T, M>, rs: RS) -> Self {
        Self { streamer, rs }
    }
    #[inline(always)]
    fn send_data(&mut self, words: DataFormat<'_>) -> Result {
        match words {
            DataFormat::U8(slice) => {
                self.streamer.stream_u8(&mut slice.iter().cloned());
                Ok(())
            }
            DataFormat::U16(slice) => {
                self.streamer
                    .stream_u16(&mut slice.iter().cloned(), |v| v.to_ne_bytes());
                Ok(())
            }
            DataFormat::U16LE(slice) => {
                self.streamer
                    .stream_u16(&mut slice.iter().cloned(), |v| v.to_le_bytes());
                Ok(())
            }
            DataFormat::U16BE(slice) => {
                self.streamer
                    .stream_u16(&mut slice.iter().cloned(), |v| v.to_be_bytes());
                Ok(())
            }
            DataFormat::U8Iter(iter) => {
                self.streamer.stream_u8(iter);
                Ok(())
            }
            DataFormat::U16LEIter(iter) => {
                self.streamer.stream_u16(iter, |v| v.to_le_bytes());
                Ok(())
            }
            DataFormat::U16BEIter(iter) => {
                self.streamer.stream_u16(iter, |v| v.to_be_bytes());
                Ok(())
            }
            _ => Err(DisplayError::DataFormatNotImplemented),
        }
    }
}

impl<'d, T, M, RS> WriteOnlyDataCommand for SpiDmaCMInterface<'d, T, M, RS>
where
    T: InstanceDma,
    M: Mode,
    RS: OutputPin,
{
    #[inline(always)]
    fn send_commands(&mut self, cmd: DataFormat<'_>) -> core::result::Result<(), DisplayError> {
        self.rs.set_low().map_err(|_| DisplayError::RSError)?;
        self.send_data(cmd)?;
        Ok(())
    }
    #[inline(always)]
    fn send_data(&mut self, buf: DataFormat<'_>) -> core::result::Result<(), DisplayError> {
        self.rs.set_high().map_err(|_| DisplayError::RSError)?;
        self.send_data(buf)?;
        Ok(())
    }
}
