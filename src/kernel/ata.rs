use bit_field::BitField;
use crate::{print, kernel};
use lazy_static::lazy_static;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

#[repr(u16)]
enum Command {
    Read = 0x20,
    Write = 0x30,
    Identify = 0xEC,
}

#[allow(dead_code)]
#[repr(usize)]
enum Status {
    ERR = 0,
    IDX = 1,
    CORR = 2,
    DRQ = 3,
    SRV = 4,
    DF = 5,
    RDY = 6,
    BSY = 7
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Bus {
    id: u8,
    irq: u8,

    data_register: Port<u16>,
    error_register: PortReadOnly<u8>,
    features_register: PortWriteOnly<u8>,
    sector_count_register: Port<u8>,
    lba0_register: Port<u8>,
    lba1_register: Port<u8>,
    lba2_register: Port<u8>,
    drive_register: Port<u8>,
    status_register: PortReadOnly<u8>,
    command_register: PortWriteOnly<u8>,

    alternate_status_register: PortReadOnly<u8>,
    control_register: PortWriteOnly<u8>,
    drive_blockess_register: PortReadOnly<u8>,
}

impl Bus {
    pub fn new(id: u8, io_base: u16, ctrl_base: u16, irq: u8) -> Self {
        Self {
            id, irq,

            data_register: Port::new(io_base + 0),
            error_register: PortReadOnly::new(io_base + 1),
            features_register: PortWriteOnly::new(io_base + 1),
            sector_count_register: Port::new(io_base + 2),
            lba0_register: Port::new(io_base + 3),
            lba1_register: Port::new(io_base + 4),
            lba2_register: Port::new(io_base + 5),
            drive_register: Port::new(io_base + 6),
            status_register: PortReadOnly::new(io_base + 7),
            command_register: PortWriteOnly::new(io_base + 7),

            alternate_status_register: PortReadOnly::new(ctrl_base + 0),
            control_register: PortWriteOnly::new(ctrl_base + 0),
            drive_blockess_register: PortReadOnly::new(ctrl_base + 1),
        }
    }

    fn reset(&mut self) {
        unsafe {
            self.control_register.write(4);
            self.control_register.write(0);
        }
    }

    fn wait(&mut self) {
        unsafe {
            self.status_register.read();
            self.status_register.read();
            self.status_register.read();
            self.status_register.read();
        }
    }

    fn write_command(&mut self, cmd: Command) {
        unsafe { self.command_register.write(cmd as u8); }
        //unsafe { self.command_register.write(cmd as u32); }
    }

    fn status(&mut self) -> u8 {
        unsafe { self.status_register.read() }
    }

    fn lba1(&mut self) -> u8 {
        unsafe { self.lba1_register.read() }
    }

    fn lba2(&mut self) -> u8 {
        unsafe { self.lba2_register.read() }
    }

    fn read_data(&mut self) -> u16 {
        unsafe { self.data_register.read() }
    }

    fn write_data(&mut self, data: u16) {
        unsafe { self.data_register.write(data) }
    }

    fn is_busy(&mut self) -> bool {
        self.status().get_bit(Status::BSY as usize)
    }

    fn is_error(&mut self) -> bool {
        self.status().get_bit(Status::ERR as usize)
    }

    fn is_ready(&mut self) -> bool {
        self.status().get_bit(Status::RDY as usize)
    }

    fn select_drive(&mut self, drive: u8) {
        // Drive #0 (primary) = 0xA0
        // Drive #1 (secondary) = 0xB0
        let drive_id = 0xA0 | (drive << 4);
        unsafe {
            self.drive_register.write(drive_id);
        }
    }

    #[allow(dead_code)]
    fn debug(&mut self) {
        self.wait();
        unsafe { print!("drive register: 0b{:08b}\n", self.drive_register.read()); }
        unsafe { print!("status:         0b{:08b}\n", self.status_register.read()); }
    }

    fn setup(&mut self, drive: u8, block: u32) {
        let drive_id = 0xE0 | (drive << 4);
        unsafe {
            self.drive_register.write(drive_id | ((block.get_bits(24..28) as u8) & 0x0F));
            self.sector_count_register.write(1);
            self.lba0_register.write(block.get_bits(0..8) as u8);
            self.lba1_register.write(block.get_bits(8..16) as u8);
            self.lba2_register.write(block.get_bits(16..24) as u8);
        }
    }

    pub fn identify_drive(&mut self, drive: u8) -> Option<[u16; 256]> {
        self.reset();
        self.wait();
        self.select_drive(drive);
        unsafe {
            self.sector_count_register.write(0);
            self.lba0_register.write(0);
            self.lba1_register.write(0);
            self.lba2_register.write(0);
        }

        self.write_command(Command::Identify);

        if self.status() == 0 {
            return None;
        }

        while self.is_busy() {}

        if self.lba1() != 0 || self.lba2() != 0 {
            return None;
        }

        for i in 0.. {
            if i == 256 {
                self.reset();
                return None;
            }
            if self.is_error() {
                return None;
            }
            if self.is_ready() {
                break
            }
        }

        let mut res = [0; 256];
        for i in 0..256 {
            res[i] = self.read_data();
        }
        Some(res)
    }

    pub fn read(&mut self, drive: u8, block: u32, buf: &mut [u8]) {
        self.setup(drive, block);

        self.write_command(Command::Read);

        while self.is_busy() {}

        for i in 0..256 {
            let data = self.read_data();
            buf[i * 2] = data.get_bits(0..8) as u8;
            buf[i * 2 + 1] = data.get_bits(8..16) as u8;
        }
    }

    pub fn write(&mut self, drive: u8, block: u32, buf: &[u8]) {
        self.setup(drive, block);

        self.write_command(Command::Write);

        while self.is_busy() {}

        for i in 0..256 {
            let mut data = 0 as u16;
            data.set_bits(0..8, buf[i * 2] as u16);
            data.set_bits(8..16, buf[i * 2 + 1] as u16);
            self.write_data(data);
        }

        while self.is_busy() {}
    }
}

lazy_static! {
    pub static ref ATA_BUSES: Mutex<Vec<Bus>> = Mutex::new(Vec::new());
}

fn disk_size(sectors: u32) -> (u32, String) {
    let bytes = sectors * 512;
    if bytes >> 20 < 1000 {
        (bytes >> 20, String::from("MB"))
    } else {
        (bytes >> 30, String::from("GB"))
    }
}

pub fn init() {
    let mut buses = ATA_BUSES.lock();
    buses.push(Bus::new(0, 0x1F0, 0x3F6, 14));
    buses.push(Bus::new(1, 0x170, 0x376, 15));

    let bus = 1;
    let drive = 0;
    if let Some(buf) = buses[bus].identify_drive(drive) {
        let mut serial = String::new();
        for i in 10..20 {
            for &b in &buf[i].to_be_bytes() {
                serial.push(b as char);
            }
        }
        let mut model = String::new();
        for i in 27..47 {
            for &b in &buf[i].to_be_bytes() {
                model.push(b as char);
            }
        }
        let sectors = (buf[61] as u32) << 16 | (buf[60] as u32);
        let (size, unit) = disk_size(sectors);
        let uptime = kernel::clock::clock_monotonic();
        print!("[{:.6}] ATA {}:{} {} {} ({} {})\n", uptime, bus, drive, model.trim(), serial.trim(), size, unit);
    }

    /*
    let block = 1;
    let mut buf = [0u8; 512];
    buses[1].read(drive, block, &mut buf);
    for i in 0..256 {
        if i % 8 == 0 {
            print!("\n{:08X} ", i * 2);
        }
        print!("{:02X}{:02X} ", buf[i * 2], buf[i * 2 + 1]);
    }
    print!("\n");

    buf[0x42] = 'H' as u8;
    buf[0x43] = 'e' as u8;
    buf[0x44] = 'l' as u8;
    buf[0x45] = 'l' as u8;
    buf[0x46] = 'o' as u8;
    for i in 0..256 {
        if i % 8 == 0 {
            print!("\n{:08X} ", i * 2);
        }
        print!("{:02X}{:02X} ", buf[i * 2], buf[i * 2 + 1]);
    }
    print!("\n");
    buses[1].write(drive, block, &mut buf);

    let mut buf = [0u8; 512];
    buses[1].read(drive, block, &mut buf);
    for i in 0..256 {
        if i % 8 == 0 {
            print!("\n{:08X} ", i * 2);
        }
        print!("{:02X}{:02X} ", buf[i * 2], buf[i * 2 + 1]);
    }
    print!("\n");
    */
}

pub fn read(bus: u8, drive: u8, block: u32, mut buf: &mut [u8]) {
    let mut buses = ATA_BUSES.lock();
    buses[bus as usize].read(drive, block, &mut buf);
}

pub fn write(bus: u8, drive: u8, block: u32, buf: &[u8]) {
    let mut buses = ATA_BUSES.lock();
    buses[bus as usize].write(drive, block, &buf);
}