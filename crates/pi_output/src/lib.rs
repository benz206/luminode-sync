/// LED output abstraction.
///
/// Compiles to a real ws2812 driver on ARM Linux (Raspberry Pi) and falls
/// back to a terminal simulator everywhere else.  This lets you develop and
/// test the full lighting pipeline on a Mac or x86 Linux machine.

use anyhow::Result;

/// A single RGB pixel.
#[derive(Clone, Copy, Default, Debug)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Pixel { r, g, b }
    }
}

/// Trait implemented by all output backends.
pub trait LedOutput: Send {
    fn write(&mut self, pixels: &[Pixel]) -> Result<()>;
    fn led_count(&self) -> usize;
}

// ─── Real hardware output (Pi only) ──────────────────────────────────────────

#[cfg(all(target_arch = "arm", target_os = "linux"))]
pub mod hardware {
    use super::*;
    use ws281x_rpi::Ws2812Rpi;

    const GPIO_PIN: i32 = 18;
    const FREQUENCY: u32 = 800_000;
    const DMA_CHANNEL: i32 = 10;
    const BRIGHTNESS: u8 = 255;

    pub struct HardwareOutput {
        strip: Ws2812Rpi,
        count: usize,
    }

    impl HardwareOutput {
        pub fn new(count: usize) -> Result<Self> {
            // Requires root / CAP_SYS_RAWIO for GPIO/DMA access.
            let strip = Ws2812Rpi::new(count as i32, GPIO_PIN, FREQUENCY, DMA_CHANNEL, BRIGHTNESS)
                .map_err(|e| anyhow::anyhow!("ws2812 init failed: {e:?}"))?;
            Ok(HardwareOutput { strip, count })
        }
    }

    impl LedOutput for HardwareOutput {
        fn write(&mut self, pixels: &[Pixel]) -> Result<()> {
            use smart_leds_trait::SmartLedsWrite;
            let colors = pixels.iter().map(|p| smart_leds_trait::RGB8 {
                r: p.r,
                g: p.g,
                b: p.b,
            });
            self.strip
                .write(colors)
                .map_err(|e| anyhow::anyhow!("LED write failed: {e:?}"))?;
            Ok(())
        }

        fn led_count(&self) -> usize {
            self.count
        }
    }
}

// ─── Simulated output (all other platforms) ───────────────────────────────────

pub mod sim {
    use super::*;

    /// Prints a compact ASCII bar chart to stderr at ~10 fps.
    /// Useful for visualising effects during development.
    pub struct SimulatedOutput {
        count: usize,
        frame_number: u64,
        /// Only render every N frames to avoid flooding the terminal.
        print_every: u64,
    }

    impl SimulatedOutput {
        pub fn new(count: usize) -> Self {
            SimulatedOutput { count, frame_number: 0, print_every: 6 }
        }
    }

    impl LedOutput for SimulatedOutput {
        fn write(&mut self, pixels: &[Pixel]) -> Result<()> {
            self.frame_number += 1;
            if self.frame_number % self.print_every != 0 {
                return Ok(());
            }

            // Print a compact energy bar using block characters.
            let n = pixels.len().min(64); // terminal width guard
            let sample_step = pixels.len() / n.max(1);

            let bar: String = (0..n)
                .map(|i| {
                    let p = &pixels[i * sample_step];
                    let luma = p.r as u32 + p.g as u32 + p.b as u32;
                    match luma / 32 {
                        0      => ' ',
                        1      => '▁',
                        2      => '▂',
                        3      => '▃',
                        4      => '▄',
                        5      => '▅',
                        6      => '▆',
                        7      => '▇',
                        _      => '█',
                    }
                })
                .collect();

            eprint!("\r[{bar}]");
            Ok(())
        }

        fn led_count(&self) -> usize {
            self.count
        }
    }
}

/// Construct the appropriate output backend for the current platform.
pub fn create_output(count: usize) -> Result<Box<dyn LedOutput>> {
    #[cfg(all(target_arch = "arm", target_os = "linux"))]
    {
        Ok(Box::new(hardware::HardwareOutput::new(count)?))
    }
    #[cfg(not(all(target_arch = "arm", target_os = "linux")))]
    {
        eprintln!("[pi_output] Non-Pi target: using simulated output");
        Ok(Box::new(sim::SimulatedOutput::new(count)))
    }
}
