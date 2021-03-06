use std::f32;
use std::io::Cursor;

use gfx;
use gfx::traits::FactoryExt;
use gfx::Factory;
use gfx_device_gl::CommandBuffer;
use gfx_device_gl::Resources;

use crate::devices::*;
use crate::pipeline::*;

const DIOPTRES_SCALING: f32 = 0.332_763_369_417_523 as f32;

gfx_defines! {
    pipeline pipe {
        u_stereo: gfx::Global<i32> = "u_stereo",
        u_active: gfx::Global<i32> = "u_active",
        u_samplecount: gfx::Global<i32> = "u_samplecount",
        u_depth_min: gfx::Global<f32> = "u_depth_min",
        u_depth_max: gfx::Global<f32> = "u_depth_max",
        // smallest distance on which the eye can focus, in mm
        u_near_point: gfx::Global<f32> = "u_near_point",
        // largest  distance on which the eye can focus, in mm
        u_far_point: gfx::Global<f32> = "u_far_point",
        // determines the bluriness of objects that are too close to focus
        // should be between 0 and 2
        u_near_vision_factor: gfx::Global<f32> = "u_near_vision_factor",
        // determines the bluriness of objects that are too far to focus
        // should be between 0 and 2
        u_far_vision_factor: gfx::Global<f32> = "u_far_vision_factor",
        s_color: gfx::TextureSampler<[f32; 4]> = "s_color",
        s_depth: gfx::TextureSampler<f32> = "s_depth",
        s_normal: gfx::TextureSampler<[f32; 4]> = "s_normal",
        s_cornea: gfx::TextureSampler<[f32; 4]> = "s_cornea",
        rt_color: gfx::RenderTarget<ColorFormat> = "rt_color",
    }
}

pub struct Lens {
    pso: gfx::PipelineState<Resources, pipe::Meta>,
    pso_data: pipe::Data<Resources>,
}

impl Lens {
    pub fn new(factory: &mut gfx_device_gl::Factory) -> Self {
        let pso = factory
            .create_pipeline_simple(
                &include_glsl!("../shader.vert"),
                &include_glsl!("shader.frag"),
                pipe::new(),
            )
            .unwrap();

        //TODO: this is one stupid and slow hack!!! pre-compute this properly
        let filename_normal = Cursor::new(include_bytes!("normal.png").to_vec());
        let (_, normal_view) = load_highres_normalmap(factory, filename_normal).unwrap();

        let rgba_cornea = vec![127; 4].into_boxed_slice();
        let (_, cornea_view) = load_texture_from_bytes(factory, rgba_cornea, 1, 1).unwrap();

        let sampler = factory.create_sampler_linear();
        let (_, src, dst) = factory.create_render_target(1, 1).unwrap();

        let (_, srv, _): (
            _,
            _,
            gfx::handle::RenderTargetView<gfx_device_gl::Resources, f32>,
        ) = factory.create_render_target(1, 1).unwrap();

        Lens {
            pso,
            pso_data: pipe::Data {
                u_stereo: 0,
                u_active: 0,
                u_samplecount: 4,
                u_depth_min: 200.0,  //XXX: was 1000.0 - 300.0,
                u_depth_max: 5000.0, //XXX: was 1000.0 + 0.0,
                u_near_point: 0.0,
                u_far_point: f32::INFINITY,
                u_near_vision_factor: 0.0,
                u_far_vision_factor: 0.0,
                s_color: (src, sampler.clone()),
                s_depth: (srv, sampler.clone()),
                s_normal: (normal_view, sampler.clone()),
                s_cornea: (cornea_view, sampler),
                rt_color: dst,
            },
        }
    }
}

impl Pass for Lens {
    fn update_io(
        &mut self,
        target: &DeviceTarget,
        _target_size: (u32, u32),
        source: &DeviceSource,
        source_sampler: &gfx::handle::Sampler<Resources>,
        _source_size: (u32, u32),
        stereo: bool,
    ) {
        self.pso_data.u_stereo = if stereo { 1 } else { 0 };
        self.pso_data.rt_color = target.clone();
        match source {
            DeviceSource::Rgb { rgba8 } => {
                self.pso_data.s_color = (rgba8.clone(), source_sampler.clone());
            }
            DeviceSource::RgbDepth { rgba8, d } => {
                self.pso_data.s_color = (rgba8.clone(), source_sampler.clone());
                self.pso_data.s_depth = (d.clone(), source_sampler.clone());
            }
            DeviceSource::Yuv { .. } => panic!("Unsupported source"),
        }
    }

    fn update_params(&mut self, _factory: &mut gfx_device_gl::Factory, params: &ValueMap) {
        // default values
        self.pso_data.u_near_point = 0.0;
        self.pso_data.u_far_point = f32::INFINITY;
        self.pso_data.u_near_vision_factor = 0.0;
        self.pso_data.u_far_vision_factor = 0.0;
        self.pso_data.u_active = 0;

        if let Some(Value::Bool(true)) = params.get("presbyopia_onoff") {
            // near point is a parameter between 0 and 100 that is to be scaled to 0 - 1000
            if let Some(Value::Number(near_point)) = params.get("presbyopia_near_point") {
                self.pso_data.u_active = 1;
                self.pso_data.u_near_point = (near_point * 10.0) as f32;
                self.pso_data.u_near_vision_factor = 1.0;
            }
        }

        if let Some(Value::Bool(true)) = params.get("myopiahyperopia_onoff") {
            if let Some(Value::Number(mnh)) = params.get("myopiahyperopia_mnh") {
                self.pso_data.u_active = 1;
                // mnh represents a range of -3D to 3D
                let dioptres = ((mnh / 50.0 - 1.0) * 3.0) as f32;

                if dioptres < 0.0 {
                    // myopia
                    self.pso_data.u_far_point = -1000.0 / dioptres;
                    // u_near_point should not be farther than u_far_point
                    self.pso_data.u_near_point =
                        self.pso_data.u_near_point.min(self.pso_data.u_far_point);
                    let vision_factor = 1.0 - dioptres * DIOPTRES_SCALING;
                    self.pso_data.u_far_vision_factor =
                        self.pso_data.u_far_vision_factor.max(vision_factor as f32);
                } else if dioptres > 0.0 {
                    // hyperopia
                    let hyperopia_near_point = 1000.0 / (4.4 - dioptres);
                    self.pso_data.u_near_point =
                        self.pso_data.u_near_point.max(hyperopia_near_point);
                    let vision_factor = 1.0 + dioptres * DIOPTRES_SCALING;
                    self.pso_data.u_near_vision_factor =
                        self.pso_data.u_near_vision_factor.max(vision_factor as f32);
                }
            }
        }
    }

    fn render(&mut self, encoder: &mut gfx::Encoder<Resources, CommandBuffer>, _: &DeviceGaze) {
        encoder.draw(&gfx::Slice::from_vertex_count(6), &self.pso, &self.pso_data);
    }
}
