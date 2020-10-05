//! Feeds back the input stream directly into the output stream.
//!
//! Assumes that the input and output devices can use the same stream configuration and that they
//! support the f32 sample format.
//!
//! Uses a delay of `LATENCY_MS` milliseconds in case the default input and output streams are not
//! precisely synchronised.

extern crate anyhow;
extern crate cpal;
extern crate ringbuf;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::RingBuffer;

const LATENCY_MS: f32 = 150.0;


//https://netwerkt.wordpress.com/2011/08/25/goertzel-filter/


#[derive(Debug)]
struct Goertzel {
  s_prev: [f32; 2],
  s_prev2: [f32; 2],
  totalpower: [f32; 2],
  freq: f32,
  samplef: f32,
  n_total: i32,
  double: f32,
  power: f32,
  s: f32,
  active: usize,
  n: [i32; 2],

}

impl Goertzel {
  fn new(freq: f32, samplef: f32) -> Self {
    Self {
      s_prev: [0., 0.],
      s_prev2: [0., 0.],
      totalpower: [0., 0.],
      freq: freq,
      samplef: samplef,
      n_total: 0,
      double: 0.,
      power: 0.,
      s: 0.,
      active: 0,
      n: [0, 0],
    }
  }
  fn filter (&mut self, sample: f32) -> f32 {
    let normalizedfreq: f32 = self.freq/self.samplef;
    let coeff: f32 = 2.*(2.*3.13*normalizedfreq).cos();
    let mut s = sample + coeff * self.s_prev[0] - self.s_prev2[0];
    self.s_prev2[0] = self.s_prev[0];
    self.s_prev[0] = s;
    self.n[0] += 1;
    s = sample + coeff * self.s_prev[1] - self.s_prev2[1];
    self.s_prev2[1] = self.s_prev[1];
    self.s_prev[1] = s;
    self.n[1] += 1;
    self.n_total += 1;
    self.active = ((self.n_total / 1000) & 0x01) as usize;

    let activen = 1-self.active as usize;

    if self.n[activen] >= 1000 {
      self.s_prev[activen] = 0.0;
      self.s_prev2[activen] = 0.0;
      self.totalpower[activen] = 0.0;
      self.n[activen] = 0;
    }
    self.totalpower[0] += sample*sample;
    self.totalpower[1] += sample*sample;

    let power = self.s_prev2[self.active] * self.s_prev2[self.active] + self.s_prev[self.active]
      * self.s_prev[self.active] - coeff * self.s_prev[self.active] * self.s_prev2[self.active];
    power / (self.totalpower[self.active]+1e-7) / (self.n[self.active] as f32)
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exploration() {
    //assert_eq!(2 + 2, 4);
    let _x = Goertzel::new(440., 44e3);
  }
}


fn main() -> Result<(), anyhow::Error> {
    // Conditionally compile with jack if the feature is specified.
    #[cfg(all(
        any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd"),
        feature = "jack"
    ))]
    // Manually check for flags. Can be passed through cargo with -- e.g.
    // cargo run --release --example beep --features jack -- --jack
    let host = if std::env::args()
        .collect::<String>()
        .contains(&String::from("--jack"))
    {
        cpal::host_from_id(cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect(
                "make sure --features jack is specified. only works on OSes where jack is available",
            )).expect("jack host unavailable")
    } else {
        cpal::default_host()
    };

    #[cfg(any(
        not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd")),
        not(feature = "jack")
    ))]
    let host = cpal::default_host();

    // Default devices.
    let input_device = host
        .default_input_device()
        .expect("failed to get default input device");
    let output_device = host
        .default_output_device()
        .expect("failed to get default output device");
    println!("Using default input device: \"{}\"", input_device.name()?);
    println!("Using default output device: \"{}\"", output_device.name()?);

    // We'll try and use the same configuration between streams to keep it simple.
    let config: cpal::StreamConfig = input_device.default_input_config()?.into();

    // Create a delay in case the input and output devices aren't synced.
    let latency_frames = (LATENCY_MS / 1_000.0) * config.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * config.channels as usize;

    // The buffer to share samples
    let ring = RingBuffer::new(latency_samples * 2);
    let (mut producer, _consumer) = ring.split();

    // Fill the samples with 0.0 equal to the length of the delay.
    for _ in 0..latency_samples {
        // The ring buffer has twice as much space as necessary to add latency here,
        // so this should never fail
        producer.push(0.0).unwrap();
    }


    let mut gfilter = Goertzel::new(440., 44e3);

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for &sample in data {
            let res = gfilter.filter(sample);
            println!("{:?}", res);
            //println!("{:?}", sample);
        }
    };

    // Build streams.
    println!(
        "Attempting to build both streams with f32 samples and `{:?}`.",
        config
    );
    let input_stream = input_device.build_input_stream(&config, input_data_fn, err_fn)?;
    println!("Successfully built streams.");

    // Play the streams.
    println!(
        "Starting the input and output streams with `{}` milliseconds of latency.",
        LATENCY_MS
    );
    input_stream.play()?;

    // Run for 3 seconds before closing.
    println!("Playing for 3 seconds... ");
    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(input_stream);
    println!("Done!");
    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}