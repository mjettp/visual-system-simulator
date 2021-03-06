#[cfg(feature = "video")]
extern crate av;
extern crate clap;

mod cmd;
mod devices;

use vss::*;

use crate::cmd::*;

fn resolve_desktop_devices(config: &Config) -> Option<Box<dyn Device>> {
    match config.device.as_ref() as &str {
        #[cfg(feature = "video")]
        "video" => {
            use crate::devices::*;
            Some(Box::new(AvDevice::new(&config)) as Box<dyn Device>)
        }
        _ => None,
    }
}

pub fn main() {
    let config = cmd_parse();

    let (mut device, mut pipeline) = config.build(&resolve_desktop_devices).unwrap();

    let mut done = false;
    while !done {
        device.begin_frame();
        pipeline.render(&mut device);
        device.end_frame(&mut done);
    }
}
