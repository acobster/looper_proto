use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

fn main() -> anyhow::Result<()> {
    // Set up an audio Device.
    let host = cpal::default_host();

    let input = host.default_input_device()
        .expect("no input device available.");
    let output = host.default_output_device()
        .expect("no output device available.");
    println!("Input device: {}", input.name()?);
    println!("Output device: {}", output.name()?);

    let config: cpal::StreamConfig = output.default_input_config()?.into();
    println!("Output config:  {:?}", config);

    // Design notes:
    //
    // Below is our vector of samples. Each line between the [ square brackets ]
    // is a loop; each dot is a sample. This is many thousands of times
    // below the number of samples we actually deal with in a buffer, but this
    // is fine for an illustration.
    //
    // [
    //  .....
    //  .....
    //  .....
    //  ...
    //    ^
    //    |
    //    +----- this is where the current recording ends, and is where the
    //           looper playback index is in this example.
    //
    //
    //
    // ]
    //
    // loop_len = 5
    // loop_count = 3
    //
    // playback:
    // sample_idx = 0..loop_len-1

    let mut looper = Looper::new();
    let input_state = looper.state.clone();
    let mut output_state = looper.state.clone();

    let (producer, consumer) = mpsc::channel::<Clip>();

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        if !input_state.is_recording.load(Ordering::SeqCst) {
            // We're not recording, save nothing.
            return;
        }

        let idx = input_state.total_samples.load(Ordering::SeqCst);
        let _ = producer.send(Clip::new(data.to_vec(), idx));
    };
    let input_stream = input.build_input_stream(&config, input_data_fn, err_fn)?;

    // Setup output callback & stream.
    let mut bank = SampleBank::new(vec![0.0; 44100 * 1000]);
    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {

        let len = output_state.loop_len.load(Ordering::SeqCst);
        let total_samples = output_state.total_samples.load(Ordering::SeqCst);

        // TODO
        // pop Option<Vec> off the queue
        // increase loop_len if first loop
        // concat samples
        match consumer.try_recv() {
            Ok(clip) => {
                if output_state.is_recording.load(Ordering::SeqCst) {
                    //println!("clip of length {} at idx {}", clip.samples.len(), clip.start);
                    bank.write_at(clip.start, &clip.samples);
                    // Update state to account for newly recorded samples.
                    output_state.total_samples.store(total_samples + clip.samples.len(), Ordering::SeqCst);
                    if output_state.is_first_loop.load(Ordering::SeqCst) {
                        output_state.loop_len.store(len + clip.samples.len(), Ordering::SeqCst);
                    }
                }
            },
            Err(_) => {
                // No new clips
            },
        }

        if output_state.is_first_loop.load(Ordering::SeqCst) {
            // Bail; no playback yet.
            return;
        }

        // Load the new loop_len
        let len = output_state.loop_len.load(Ordering::SeqCst);
        let total_samples = output_state.total_samples.load(Ordering::SeqCst);
        let loop_count = div_ceil(total_samples, len);
        //println!("loop_count = {} / {} = {}, output_idx={}", total_samples, len, loop_count, output_idx);

        // Still possible there are no clips yet, in which case we don't
        // need to do anything.
        if loop_count < 1 {
            return;
        }

        for sample in data {
            // Sum up all samples at each corresponding index across loops.
            let mut sum = 0.0;
            for loop_offset in 0..(loop_count - 1) {
                let sample_idx = output_state.get_playback() + len * loop_offset;
                sum += bank.samples[sample_idx];
            }
            // TODO dynamic range compression!
            *sample = sum;

            output_state.advance_playback();
        }
    };
    let output_stream = output.build_output_stream(&config, output_data_fn, err_fn)?;

    looper.input = Some(input_stream);
    looper.output = Some(output_stream);

    init_ui(looper);

    Ok(())
}

struct SampleBank {
    samples: Vec<f32>,
}

impl SampleBank {
    fn new(samples: Vec<f32>) -> Self {
        Self {
            samples: samples,
        }
    }

    // Write new samples contiguously to this SampleBank, starting at idx
    fn write_at(&mut self, mut idx: usize, samples: &Vec<f32>) {
        for sample in samples {
            self.samples[idx] = *sample;
            idx += 1;
        }
    }
}

// TODO different implementations of this for different platforms.
// This should be the only platform-specific feature.
fn init_ui(mut looper: Looper) {
    let mut line = String::new();
    println!("Hit ENTER to start recording.");
    loop {
        let _ = std::io::stdin().read_line(&mut line).unwrap();
        looper.tap().expect("tap failed!");
    }
}

#[derive(Clone)]
struct State {
    playback: Arc<AtomicUsize>,
    loop_len: Arc<AtomicUsize>,
    total_samples: Arc<AtomicUsize>,
    is_recording: Arc<AtomicBool>,
    is_first_loop: Arc<AtomicBool>,
}

impl State {
    fn new() -> Self {
        Self {
            playback: Arc::new(0.into()),
            loop_len: Arc::new(0.into()),
            total_samples: Arc::new(0.into()),
            is_recording: Arc::new(false.into()),
            is_first_loop: Arc::new(true.into()),
        }
    }

    fn recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }

    fn first_loop(&self) -> bool {
        self.is_first_loop.load(Ordering::SeqCst)
    }

    fn began_recording(&self) -> bool {
        self.recording() || !self.first_loop()
    }

    fn get_playback(&self) -> usize {
        self.playback.load(Ordering::SeqCst)
    }

    fn get_loop_len(&self) -> usize {
        self.loop_len.load(Ordering::SeqCst)
    }

    fn advance_playback(&mut self) {
        if !self.began_recording() {
            return;
        }

        let mut playback = self.get_playback();
        playback += 1;

        if playback >= self.get_loop_len() {
            playback = 0;
        }

        self.playback.store(playback, Ordering::SeqCst);
    }
}

struct Clip {
    samples: Vec<f32>,
    start: usize,
}

impl Clip {
    fn new(samples: Vec<f32>, start: usize) -> Self {
        Self {
            samples: samples,
            start: start,
        }
    }
}

struct Looper {
    pub state: State,
    pub input: Option<cpal::Stream>,
    pub output: Option<cpal::Stream>,

    pub tap_count: usize,
}

impl Looper {
    fn new() -> Self {
        Self {
            state: State::new(),
            input: None,
            output: None,
            tap_count: 0,
        }
    }

    fn tap(&mut self) -> anyhow::Result<()> {
        match self.tap_count {
            0 => {
                println!("RECORDING.");
                self.state.is_recording.store(true, Ordering::SeqCst);
                self.output.as_ref().unwrap().play()?;
                self.input.as_ref().unwrap().play()?;
            },
            1 => {
                println!("SET FIRST LOOP LENGTH.");
                self.state.is_first_loop.store(false, Ordering::Release);
            },
            _ => {
                let is_recording = self.state.is_recording.load(Ordering::SeqCst);
                self.state.is_recording.store(!is_recording, Ordering::Release);
                println!("recording={}", !is_recording);
            },
        }
        self.tap_count += 1;
        Ok(())
    }
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}

#[inline]
fn div_ceil(first: usize, other: usize) -> usize {
    if other == 0 {
        0
    } else if (first % other) > 0 && other > 0 {
        first / other + 1
    } else {
        first / other
    }
}
