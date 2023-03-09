use clap::{AppSettings, Arg, Command};
use ravif::{load_rgba, AlphaColorMode, BoxError, ColorSpace, EncodedImage, Encoder};
use rayon::prelude::*;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        let mut source = e.source();
        while let Some(e) = source {
            eprintln!("  because: {e}");
            source = e.source();
        }
        std::process::exit(1);
    }
}

enum MaybePath {
    Stdio,
    Path(PathBuf),
}

fn run() -> Result<(), BoxError> {
    let args = Command::new("cavif-rs")
        .version(clap::crate_version!())
        .author("Kornel Lesiński <kornel@imageoptim.com>")
        .about("Convert JPEG/PNG images to AVIF image format (based on AV1/rav1e)")
        .setting(AppSettings::DeriveDisplayOrder)
        .arg(Arg::new("quality")
            .short('Q')
            .long("quality")
            .value_name("n")
            .help("Quality from 1 (worst) to 100 (best)")
            .default_value("80")
            .takes_value(true))
        .arg(Arg::new("speed")
            .short('s')
            .long("speed")
            .value_name("n")
            .default_value("4")
            .help("Encoding speed from 0 (best) to 10 (fast but ugly)")
            .takes_value(true))
        .arg(Arg::new("threads")
            .short('j')
            .long("threads")
            .value_name("n")
            .default_value("0")
            .help("Maximum threads to use (0 = one thread per host core)")
            .takes_value(true))
        .arg(Arg::new("overwrite")
            .alias("--force")
            .short('f')
            .long("overwrite")
            .help("Replace files if there's .avif already"))
        .arg(Arg::new("output")
            .short('o')
            .long("output")
            .allow_invalid_utf8(true)
            .value_name("path")
            .help("Write output to this path instead of same_file.avif. It may be a file or a directory.")
            .takes_value(true))
        .arg(Arg::new("quiet")
            .short('q')
            .long("quiet")
            .help("Don't print anything"))
        .arg(Arg::new("dirty-alpha")
            .long("dirty-alpha")
            .help("Keep RGB data of fully-transparent pixels (makes larger, lower quality files)"))
        .arg(Arg::new("color")
            .long("color")
            .default_value("ycbcr")
            .takes_value(true)
            .possible_values(["ycbcr", "rgb"])
            .help("Internal AVIF color space. YCbCr works better for human eyes."))
        .arg(Arg::new("IMAGES")
            .index(1)
            .allow_invalid_utf8(true)
            .min_values(1)
            .help("One or more JPEG or PNG files to convert. \"-\" is interpreted as stdin/stdout.")
            .multiple_occurrences(true))
        .get_matches();

    let output = args.value_of_os("output").map(|s| match s {
        s if s == "-" => MaybePath::Stdio,
        s => MaybePath::Path(PathBuf::from(s)),
    });
    let quality = args.value_of_t::<f32>("quality")?;
    let alpha_quality = ((quality + 100.) / 2.).min(quality + quality / 4. + 2.);
    let speed: u8 = args.value_of_t::<u8>("speed")?;
    let overwrite = args.is_present("overwrite");
    let quiet = args.is_present("quiet");
    let threads = args.value_of_t::<usize>("threads")?;
    let dirty_alpha = args.is_present("dirty-alpha");

    let color_space = match args.value_of("color").expect("default") {
        "ycbcr" => ColorSpace::YCbCr,
        "rgb" => ColorSpace::RGB,
        x => Err(format!("bad color type: {x}"))?,
    };
    let files = args
        .values_of_os("IMAGES")
        .ok_or("Please specify image paths to convert")?;
    let files: Vec<_> = files
        .filter(|pathstr| {
            let path = Path::new(&pathstr);
            if let Some(s) = path.to_str() {
                if quiet && s.parse::<u8>().is_ok() && !path.exists() {
                    eprintln!("warning: -q is not for quality, so '{s}' is misinterpreted as a file. Use -Q {s}");
                }
            }
            path.extension().map_or(true, |e| if e == "avif" {
                if !quiet {
                    if path.exists() {
                        eprintln!("warning: ignoring {}, because it's already an AVIF", path.display());
                    } else {
                        eprintln!("warning: Did you mean to use -o {p}?", p = path.display());
                        return true;
                    }
                }
                false
            } else {
                true
            })
        })
        .map(|p| if p == "-" {
            MaybePath::Stdio
        } else {
            MaybePath::Path(PathBuf::from(p))
        })
        .collect();

    if files.is_empty() {
        return Err("No PNG/JPEG files specified".into());
    }

    let use_dir = match output {
        Some(MaybePath::Path(ref path)) => {
            if files.len() > 1 {
                let _ = fs::create_dir_all(path);
            }
            files.len() > 1 || path.is_dir()
        }
        _ => false,
    };

    let process = move |data: Vec<u8>, input_path: &MaybePath| -> Result<(), BoxError> {
        let img = load_rgba(&data, false)?;
        drop(data);
        let out_path = match (&output, input_path) {
            (None, MaybePath::Path(input)) => MaybePath::Path(input.with_extension("avif")),
            (Some(MaybePath::Path(output)), MaybePath::Path(ref input)) => MaybePath::Path({
                if use_dir {
                    output.join(Path::new(input.file_name().unwrap()).with_extension("avif"))
                } else {
                    output.clone()
                }
            }),
            (None, MaybePath::Stdio) | (Some(MaybePath::Stdio), _) => MaybePath::Stdio,
            (Some(MaybePath::Path(output)), MaybePath::Stdio) => MaybePath::Path(output.clone()),
        };
        match out_path {
            MaybePath::Path(ref p) if !overwrite && p.exists() => {
                return Err(format!("{} already exists; skipping", p.display()).into());
            }
            _ => {}
        }
        let enc = Encoder::new()
            .with_quality(quality)
            .with_speed(speed)
            .with_alpha_quality(alpha_quality)
            .with_internal_color_space(color_space)
            .with_alpha_color_mode(if dirty_alpha {
                AlphaColorMode::UnassociatedDirty
            } else {
                AlphaColorMode::UnassociatedClean
            })
            .with_num_threads(Some(threads).filter(|&n| n > 0));
        let EncodedImage {
            avif_file,
            color_byte_size,
            alpha_byte_size,
            ..
        } = enc.encode_rgba(img.as_ref())?;
        match out_path {
            MaybePath::Path(ref p) => {
                if !quiet {
                    println!(
                        "{}: {}KB ({color_byte_size}B color, {alpha_byte_size}B alpha, {}B HEIF)",
                        p.display(),
                        (avif_file.len() + 999) / 1000,
                        avif_file.len() - color_byte_size - alpha_byte_size
                    );
                }
                fs::write(p, avif_file)
            }
            MaybePath::Stdio => std::io::stdout().write_all(&avif_file),
        }
        .map_err(|e| format!("Unable to write output image: {e}"))?;
        Ok(())
    };

    let failures = files
        .into_par_iter()
        .map(|path| {
            let tmp;
            let (data, path_str): (_, &dyn std::fmt::Display) = match path {
                MaybePath::Stdio => {
                    let mut data = Vec::new();
                    std::io::stdin().read_to_end(&mut data)?;
                    (data, &"stdin")
                }
                MaybePath::Path(ref path) => {
                    let data = fs::read(path).map_err(|e| {
                        format!("Unable to read input image {}: {e}", path.display())
                    })?;
                    tmp = path.display();
                    (data, &tmp)
                }
            };
            process(data, &path).map_err(|e| BoxError::from(format!("{path_str}: error: {e}")))
        })
        .filter_map(|res| res.err())
        .collect::<Vec<BoxError>>();

    if !failures.is_empty() {
        if !quiet {
            for f in failures {
                eprintln!("error: {f}");
            }
        }
        std::process::exit(1);
    }
    Ok(())
}
