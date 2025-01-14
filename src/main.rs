#![allow(non_snake_case)]
#![feature(try_blocks)]

use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use clap::{App, Arg};
use crate::conv::{ConvertOptions, OwnedOrMut, OwnedOrRef};
use crate::rbx::Material;

mod rbx;
mod vmf;
mod conv;

fn main() -> ExitCode {
    let matches = App::new("RBXLX2VMF")
        .version("1.0")
        .about("Converts Roblox RBXLX files to Valve VMF files.")
        .arg(Arg::with_name("input")
            .long("input")
            .short("i")
            .value_name("FILE")
            .help("Sets input file")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("output")
            .long("output")
            .short("o")
            .value_name("FILE")
            .help("Sets output file")
            .default_value("rbxlx_out.vmf")
            .takes_value(true))
        .arg(Arg::with_name("texture-output")
            .long("texture-output")
            .value_name("FOLDER")
            .help("Sets texture output folder")
            .default_value("./textures-out")
            .takes_value(true))
        .arg(Arg::with_name("no-textures")
            .long("no-textures")
            .help("disables texture generation")
            .takes_value(false))
        .arg(Arg::with_name("dev-textures")
            .long("dev-textures")
            .help("use developer textures instead of roblox textures")
            .takes_value(false))
        .arg(Arg::with_name("auto-skybox")
            .long("auto-skybox")
            .help("enables automatic skybox (Warning: Results in highly unoptimized map)")
            .takes_value(false))
        .arg(Arg::with_name("optimize")
            .long("optimize")
            .help("enables part-count reduction by joining adjacent parts")
            .takes_value(false))
        .arg(Arg::with_name("skybox-height")
            .long("skybox-height")
            .help("sets additional auto-skybox height clearance")
            .takes_value(true))
        .arg(Arg::with_name("map-scale")
            .long("map-scale")
            .help("sets map scale")
            .default_value("15")
            .takes_value(true))
        .arg(Arg::with_name("decal-size")
            .long("decal-size")
            .help("sets downloaded decal texture size")
            .default_value("256")
            .takes_value(true))
        .arg(Arg::with_name("game")
            .long("game")
            .short("g")
            .help("sets target source engine game")
            .required(true)
            .takes_value(true)
            .possible_values(&["css", "csgo", "gmod", "hl2", "hl2e1", "hl2e2", "hl", "hls", "l4d", "l4d2", "portal2", "portal", "tf2"])
        )
        .get_matches();

    let exit_code = async_std::task::block_on(
        conv::convert(CLIConvertOptions {
            input_name: matches.value_of("input").unwrap(),
            input_path: matches.value_of_os("input").unwrap(),
            output_path: matches.value_of_os("output").unwrap(),
            texture_output_folder: {
                let texture_folder = matches.value_of_os("texture-output").unwrap();
                if let Err(error) = std::fs::create_dir_all(Path::new(texture_folder).join("rbx")) {
                    println!("error: could not create texture output directory {}", error);
                    std::process::exit(-1)
                }
                texture_folder
            },
            is_texture_output_enabled: !matches.is_present("no-textures"),
            use_developer_textures: matches.is_present("dev-textures"),
            map_scale: match matches.value_of("map-scale").unwrap().parse() {
                Ok(f) => f,
                Err(_) => {
                    println!("error: invalid map scale");
                    std::process::exit(-1)
                }
            },
            auto_skybox_enabled: matches.is_present("auto-skybox"),
            skybox_clearance: matches.value_of("skybox-height").map(str::parse).and_then(Result::ok).unwrap_or(0f64),
            optimization_enabled: matches.is_present("optimize"),
            decal_size: match matches.value_of("decal-size").unwrap().parse() {
                Ok(size) => size,
                Err(_) => {
                    println!("error: invalid decal size");
                    std::process::exit(-1)
                }
            },
            skybox_name: match matches.value_of("game").unwrap() {
                "css" => "sky_day01_05",
                "csgo" => "sky_day02_05",
                "gmod" => "painted",
                "hl2" => "sky_day01_04",
                "hl2e1" => "sky_ep01_01",
                "hl2e2" => "sky_ep02_01_hdr",
                "hl" => "city",
                "hls" => "sky_wasteland02",
                "l4d" => "river_hdr",
                "l4d2" => "sky_l4d_c1_2_hdr",
                "portal2" => "sky_day01_01",
                "portal" => "sky_day01_05_hdr",
                "tf2" => "sky_day01_01",
                _ => "default_skybox_fixme" // The only guard against invalid values here is HTML form validation, but as we're a clientside application, just substitute in a placeholder value
            }
        })
    );

    return match exit_code {
        Ok(code) => ExitCode::from(code),
        // Error writing to STDIO
        Err(error) => {
            eprintln!("{}", error);
            ExitCode::FAILURE
        }
    }
}

struct CLIConvertOptions<'a> {
    input_name: &'a str,
    input_path: &'a OsStr,
    output_path: &'a OsStr,
    texture_output_folder: &'a OsStr,
    is_texture_output_enabled: bool,
    use_developer_textures: bool,
    map_scale: f64,
    auto_skybox_enabled: bool,
    skybox_clearance: f64,
    optimization_enabled: bool,
    decal_size: u64,
    skybox_name: &'a str
}

impl<'a> ConvertOptions<&'static [u8], File> for CLIConvertOptions<'a> {
    fn print_output(&self) -> Box<dyn Write> {
        Box::new(std::io::stdout())
    }
    fn error_output(&self) -> Box<dyn Write> {
        Box::new(std::io::stderr())
    }

    fn input_name(&self) -> &str {
        &self.input_name
    }

    fn read_input_data(&self) ->  OwnedOrRef<'_, String> {
        let mut file = match File::open(self.input_path) {
            Ok(file) => file,
            Err(error) => {
                println!("error: Could not open input file: {}", error);
                std::process::exit(-1)
            }
        };
        let mut buffer = String::with_capacity(file.metadata().as_ref().map(Metadata::len).unwrap_or(0) as usize);
        match file.read_to_string(&mut buffer) {
            Ok(_) => {}
            Err(error) => {
                println!("error: Could not read input {}", error);
                std::process::exit(-1)
            }
        }
        OwnedOrRef::Owned(buffer)
    }

    fn vmf_output(&mut self) -> OwnedOrMut<'_, File> {
        match File::create(self.output_path) {
            Ok(file) => OwnedOrMut::Owned(file),
            Err(error) => {
                println!("error: Could not create output file {}", error);
                std::process::exit(-1)
            }
        }
    }

    fn texture_input(&mut self, texture: Material) -> Option<OwnedOrMut<'_, &'static [u8]>> {
        Some(OwnedOrMut::Owned(match texture {
            Material::Plastic => crate::rbx::textures::PLASTIC,
            Material::Wood => crate::rbx::textures::WOOD,
            Material::Slate => crate::rbx::textures::SLATE,
            Material::Concrete => crate::rbx::textures::CONCRETE,
            Material::CorrodedMetal => crate::rbx::textures::RUST,
            Material::DiamondPlate => crate::rbx::textures::DIAMONDPLATE,
            Material::Foil => crate::rbx::textures::ALUMINIUM,
            Material::Grass => crate::rbx::textures::GRASS,
            Material::Ice => crate::rbx::textures::ICE,
            Material::Marble => crate::rbx::textures::MARBLE,
            Material::Granite => crate::rbx::textures::GRANITE,
            Material::Brick => crate::rbx::textures::BRICK,
            Material::Pebble => crate::rbx::textures::PEBBLE,
            Material::Sand => crate::rbx::textures::SAND,
            Material::Fabric => crate::rbx::textures::FABRIC,
            Material::SmoothPlastic => crate::rbx::textures::SMOOTHPLASTIC,
            Material::Metal => crate::rbx::textures::METAL,
            Material::WoodPlanks => crate::rbx::textures::WOODPLANKS,
            Material::Cobblestone => crate::rbx::textures::COBBLESTONE,
            Material::Glass => crate::rbx::textures::GLASS,
            Material::ForceField => crate::rbx::textures::FORCEFIELD,
            Material::Custom { texture: "decal", .. } => crate::rbx::textures::DECAL,
            Material::Custom { texture: "studs", .. } => crate::rbx::textures::STUDS,
            Material::Custom { texture: "inlet", .. } => crate::rbx::textures::INLET,
            Material::Custom { texture: "spawnlocation", .. } => crate::rbx::textures::SPAWNLOCATION,
            Material::Custom { .. } | Material::Decal { .. } | Material::Texture { .. } => return None,
        }))
    }

    fn texture_output(&mut self, path: &str) -> OwnedOrMut<'_, File> {
        let texture_out_path = Path::new(self.texture_output_folder).join(path);
        match File::create(texture_out_path) {
            Ok(file) => OwnedOrMut::Owned(file),
            Err(error) => {
                println!("error: Could not create file {}", error);
                std::process::exit(-1)
            }
        }
    }

    fn texture_output_enabled(&self) -> bool {
        self.is_texture_output_enabled
    }

    fn use_dev_textures(&self) -> bool {
        self.use_developer_textures
    }

    fn map_scale(&self) -> f64 {
        self.map_scale
    }

    fn auto_skybox_enabled(&self) -> bool {
        self.auto_skybox_enabled
    }

    fn skybox_clearance(&self) -> f64 {
        self.skybox_clearance
    }

    fn optimization_enabled(&self) -> bool {
        self.optimization_enabled
    }

    fn decal_size(&self) -> u64 {
        self.decal_size
    }

    fn skybox_name(&self) -> &str {
        self.skybox_name
    }

    fn web_origin(&self) -> &str {
        ""  // Unused in CLI version; TODO: Remove when async-trait functions are available.
    }
}