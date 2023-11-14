#![no_std]
#![no_main]

extern crate alloc;
use core::mem::MaybeUninit;
use embedded_svc::{
    ipv4::Interface,
    wifi::{AccessPointInfo, ClientConfiguration, Configuration, Wifi},
};
use esp_backtrace as _;
use esp_println::println;
use hal::{clock::ClockControl, peripherals::Peripherals, prelude::*, Delay};

use esp_wifi::{
    current_millis, initialize,
    wifi::{utils::create_network_interface, WifiError, WifiMode},
    wifi_interface::WifiStack,
    EspWifiInitFor,
};

use hal::{timer::TimerGroup, Rng};
use smoltcp::iface::SocketStorage;

/// Configuration file embedded in the binary.
#[toml_cfg::toml_config]
pub struct Config {
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
}

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}
#[entry]
fn main() -> ! {
    init_heap();
    let peripherals = Peripherals::take();
    let system = peripherals.SYSTEM.split();

    let clocks = ClockControl::max(system.clock_control).freeze();
    let mut delay = Delay::new(&clocks);

    // setup logger
    // To change the log_level change the env section in .cargo/config.toml
    // or remove it and set ESP_LOGLEVEL manually before running cargo run
    // this requires a clean rebuild because of https://github.com/rust-lang/cargo/issues/10358
    esp_println::logger::init_logger_from_env();
    log::info!("Logger is setup");
    let timer = TimerGroup::new(peripherals.TIMG1, &clocks).timer0;
    let init = initialize(
        EspWifiInitFor::Wifi,
        timer,
        Rng::new(peripherals.RNG),
        system.radio_clock_control,
        &clocks,
    )
    .unwrap();

    let wifi = peripherals.WIFI;
    let mut socket_set_entries: [SocketStorage; 3] = Default::default();
    let (iface, device, mut controller, sockets) =
        create_network_interface(&init, wifi, WifiMode::Sta, &mut socket_set_entries).unwrap();
    let wifi_stack = WifiStack::new(iface, device, sockets, current_millis);

    let cc = Configuration::Client(ClientConfiguration {
        ssid: CONFIG.wifi_ssid.into(),
        password: CONFIG.wifi_psk.into(),
        ..Default::default()
    });

    let res = controller.set_configuration(&cc);
    log::info!("wifi_set_configuration returned {:?}", res);

    controller.start().unwrap();

    log::info!("wifi started {:?}", controller.is_started());
    log::info!("Start wifi scan for {}", CONFIG.wifi_ssid);

    let res: Result<(heapless::Vec<AccessPointInfo, 10>, usize), WifiError> = controller.scan_n();
    if let Ok((res, _count)) = res {
        for ap in res {
            log::info!("{:?}", ap);
        }
    }

    log::info!("{:?}", controller.get_capabilities());
    log::info!("wifi_connect {:?}", controller.connect());

    // wait to get connected
    log::info!("Wait to get connected");
    loop {
        let res = controller.is_connected();
        match res {
            Ok(connected) => {
                if connected {
                    break;
                }
            }
            Err(err) => {
                log::info!("{:?}", err);
                loop {}
            }
        }
    }

    log::info!("{:?}", controller.is_connected());

    // wait for an IP
    log::info!("Wait to get an ip address");
    loop {
        wifi_stack.work();

        if wifi_stack.is_iface_up() {
            log::info!("got ip {:?}", wifi_stack.get_ip_info());
            break;
        }
    }

    // We would like to reply to a ping here.
    let mut rx_buffer = [0u8; 1536];
    let mut tx_buffer = [0u8; 1536];
    let mut socket = wifi_stack.get_socket(&mut rx_buffer, &mut tx_buffer);

    loop {
        println!("Loop...");
        socket.work();
        delay.delay_ms(500u32);
    }
}
