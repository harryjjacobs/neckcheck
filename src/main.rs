extern crate nokhwa;
extern crate rustface;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use thiserror::Error;

use rustface::{Detector, ImageData};

use image::{DynamicImage, GrayImage, Rgb, RgbImage};

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;

use console::Term;

use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::{Fullscreen, WindowBuilder},
};

#[derive(Error, Debug, Clone)]
pub enum WebCamError {
    #[error("Failed to grab a frame: {0}")]
    FrameGrabError(String),
    #[error("Failed to open camera stream: {0}")]
    StreamOpenError(String),
    #[error("Failed to close camera stream {0}")]
    StreamCloseError(String),
    #[error("Failed to decode image: {0}")]
    FrameDecodeError(String),
}

enum WebCamMode {
    Continuous,
    Discrete,
}

struct WebCam {
    camera: Camera,
    mode: WebCamMode,
}

impl WebCam {
    pub fn new(index: u32, mode: WebCamMode) -> WebCam {
        let index = CameraIndex::Index(index);
        // request the absolute highest resolution CameraFormat that can be decoded to RGB.
        let requested =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
        // make the camera
        let camera = match Camera::new(index.clone(), requested) {
            Ok(c) => c,
            Err(e) => panic!("Failed to open camera {}: {}", index.clone(), e),
        };
        WebCam { camera, mode }
    }

    // Captures a single frame from the camera
    pub fn capture(&mut self) -> Result<RgbImage, WebCamError> {
        if !self.camera.is_stream_open() {
            let _ = self.open();
        }

        // get a frame
        let frame = self
            .camera
            .frame()
            .map_err(|e| WebCamError::FrameGrabError(e.to_string()))?;
        println!("Captured Single Frame of {} bytes", frame.buffer().len());

        // decode into an ImageBuffer
        let decoded = frame
            .decode_image::<RgbFormat>()
            .map_err(|e| WebCamError::FrameDecodeError(e.to_string()))?;

        if matches!(self.mode, WebCamMode::Discrete) {
            let _ = self.close();
        }

        return Ok(decoded);
    }

    fn open(&mut self) -> Result<(), WebCamError> {
        let _ = self
            .camera
            .open_stream()
            .map_err(|e| WebCamError::StreamOpenError(e.to_string()))?;
        return Ok(());
    }

    fn close(&mut self) -> Result<(), WebCamError> {
        let _ = self
            .camera
            .stop_stream()
            .map_err(|e| WebCamError::StreamCloseError(e.to_string()))?;
        return Ok(());
    }
}

struct FaceDetector {
    detector: Box<dyn Detector>,
}

impl FaceDetector {
    pub fn new() -> FaceDetector {
        let mut detector = match rustface::create_detector("seeta_fd_frontal_v1.0.bin") {
            Ok(d) => d,
            Err(e) => panic!("Failed to create detector: {}", e),
        };
        detector.set_min_face_size(20);
        detector.set_score_thresh(2.0);
        detector.set_pyramid_scale_factor(0.8);
        detector.set_slide_window_step(4, 4);
        FaceDetector { detector }
    }

    pub fn detect(&mut self, image: &GrayImage) -> Vec<Rect> {
        let mut image = ImageData::new(image.as_raw(), image.width(), image.height());
        return self
            .detector
            .detect(&mut image)
            .iter()
            .map(|f| {
                Rect::at(f.bbox().x(), f.bbox().y()).of_size(f.bbox().width(), f.bbox().height())
            })
            .collect();
    }

    pub fn draw(image: &mut RgbImage, faces: Vec<Rect>) {
        for face in faces {
            draw_hollow_rect_mut(image, face, Rgb([255, 0, 0]));
        }
    }
}

#[derive(Debug, Clone)]
struct Size {
    width: u32,
    height: u32,
}

impl Size {
    pub fn new(width: u32, height: u32) -> Size {
        Size { width, height }
    }
}

struct NeckCheckCalibration {
    max_detection_size: Size, // the maximum allowed size of the face detection box before it is
                              // deemed that the user is too close to the camera
}

struct NeckCheck {
    webcam: WebCam,
    detector: FaceDetector,
    calibration: Option<NeckCheckCalibration>,
}

impl NeckCheck {
    pub fn new(webcam: WebCam, detector: FaceDetector) -> NeckCheck {
        NeckCheck {
            webcam,
            detector,
            calibration: None,
        }
    }

    pub fn with_calibration(
        webcam: WebCam,
        detector: FaceDetector,
        calibration: NeckCheckCalibration,
    ) -> NeckCheck {
        NeckCheck {
            webcam,
            detector,
            calibration: Some(calibration),
        }
    }

    pub fn calibrate(&mut self) {
        let term = Term::stdout();
        let _ = term.write_line("Press any key to begin calibration...");
        let _ = term.read_line();
        let mut faces = Vec::new();
        while faces.is_empty() {
            let _ = term.write_line("Move to the position that you would consider to be a bad posture and then press any key.");
            let _ = term.read_line();
            faces = self.detect();
            if faces.is_empty() {
                println!("No face was detected. Please try again.");
            }
            if faces.len() > 1 {
                println!("More than one face was detected. Please try again.");
                faces.clear();
            }
        }
        let face = faces.first().unwrap();
        let size = Size::new(face.width(), face.height());
        self.calibration = Some(NeckCheckCalibration {
            max_detection_size: size.clone(),
        });

        println!(
            "Calibration successful. Using max_detection_size: {:?}",
            size
        );
    }

    pub fn check(&mut self) -> bool {
        let faces = self.detect();
        if faces.is_empty() {
            return true;
        }
        if self.calibration.is_none() {
            panic!("No calibration!");
        }
        let face = faces.first().unwrap();
        let calib = &self.calibration.as_ref().unwrap();
        if face.width() > calib.max_detection_size.width
            || face.height() > calib.max_detection_size.height
        {
            return false;
        }
        return true;
    }

    fn detect(&mut self) -> Vec<Rect> {
        let rgb_image = self.webcam.capture().unwrap();
        let image = DynamicImage::ImageRgb8(rgb_image);
        return self.detector.detect(&image.to_luma8());
    }
}

unsafe impl Send for NeckCheck {}

fn main() {
    let neckcheck: Arc<Mutex<NeckCheck>> = Arc::new(Mutex::new(NeckCheck::new(
        WebCam::new(0, WebCamMode::Continuous),
        FaceDetector::new(),
    )));
    neckcheck.lock().unwrap().calibrate();

    let is_too_close = std::sync::Arc::new(std::sync::Mutex::new(false));

    // Create the GUI event loop
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_fullscreen(Some(Fullscreen::Borderless(None)))
        .build(&event_loop)
        .unwrap();

    // Create a thread for proximity checking
    let proximity_thread = {
        let is_too_close = is_too_close.clone();
        thread::spawn(move || {
            loop {
                let is_close = !neckcheck.lock().unwrap().check();
                *is_too_close.lock().unwrap() = is_close;
                window.set_visible(is_close);
                if is_close {
                    println!("Too close!");
                }
                // thread::sleep(Duration::from_secs(1));
            }
        })
    };

    let _ = event_loop.run(|event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            _ => (),
        }
    });

    // Wait for the proximity checking thread to finish
    proximity_thread.join().unwrap();

    // let mut rgb_image = webcam.capture().unwrap();
    // let image = DynamicImage::ImageRgb8(rgb_image.clone());
    // let faces = detector.detect(&image.to_luma8());
    //
    // FaceDetector::draw(&mut rgb_image, faces);
    //
    // match rgb_image.save("output.png") {
    //     Ok(_) => println!("Saved result to {}", "output.png"),
    //     Err(message) => println!("Failed to save result to a file. Reason: {}", message),
    // }
}
