mod archive;
mod audio;
mod codec;
mod cmdbar;
mod data;
mod menu;
mod render;
mod save;
mod saveload;
mod sdl2;
mod video;
mod vm;

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        return;
    }
    match args[1].as_str() {
        "kcap" => {
            if args.len() < 3 { eprintln!("usage: wa2-native kcap <pakfile> [outdir]"); return; }
            let out = out_dir(&args[2], &args, 3, "_extracted");
            if let Err(e) = archive::kcap::unpack(Path::new(&args[2]), Path::new(&out)) {
                eprintln!("error: {}", e); std::process::exit(1);
            }
        }
        "lac" => {
            if args.len() < 3 { eprintln!("usage: wa2-native lac <pakfile> [outdir]"); return; }
            let out = out_dir(&args[2], &args, 3, "_audio");
            if let Err(e) = archive::lac::unpack(Path::new(&args[2]), Path::new(&out)) {
                eprintln!("error: {}", e); std::process::exit(1);
            }
        }
        "extract" => {
            if args.len() < 3 { eprintln!("usage: wa2-native extract <pakfile> [outdir]"); return; }
            let path = Path::new(&args[2]);
            let mut hdr = [0u8; 4];
            if std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut hdr)).is_err() {
                eprintln!("error: cannot read {}", path.display()); std::process::exit(1);
            }
            let out = out_dir(&args[2], &args, 3, "_extracted");
            let res = if &hdr[0..4] == b"KCAP" {
                archive::kcap::unpack(path, Path::new(&out))
            } else if &hdr[0..4] == b"LAC\0" {
                archive::lac::unpack(path, Path::new(&out))
            } else {
                eprintln!("error: unknown archive magic {:?}", &hdr[0..4]); std::process::exit(1);
            };
            if let Err(e) = res { eprintln!("error: {}", e); std::process::exit(1); }
        }
        "tga" => {
            if args.len() < 3 { eprintln!("usage: wa2-native tga <file.tga>"); return; }
            let data = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); std::process::exit(1); } };
            match codec::tga::decode(&data) {
                Ok(img) => println!("{}: {} x {} RGBA ({} bytes)", args[2], img.width, img.height, img.rgba.len()),
                Err(e) => { eprintln!("tga decode error: {}", e); std::process::exit(1); }
            }
        }
        "render-test" => {
            if args.len() < 4 { eprintln!("usage: wa2-native render-test <font.tga> <out.tga> [sprite.tga]"); return; }
            let font = match codec::tga::decode(&match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } }) {
                Ok(i) => i, Err(e) => { eprintln!("tga decode error: {}", e); return; }
            };
            let sprite = if args.len() >= 5 {
                match std::fs::read(&args[4]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                    Some(s) => Some(s), None => { eprintln!("warning: could not load sprite {}", args[4]); None }
                }
            } else { None };
            let surf = render::demo_scene(&font, sprite.as_ref(), 1280, 720);
            if let Err(e) = surf.save_tga(&args[3]) { eprintln!("error writing {}: {}", args[3], e); std::process::exit(1); }
            println!("wrote scene {} ({}x{}), font atlas {}x{}", args[3], surf.width, surf.height, font.width, font.height);
        }
        "font-test" => {
            if args.len() < 5 { eprintln!("usage: wa2-native font-test <font.tga> <charset.txt> <out.tga> [text]"); return; }
            let font = match load_font_charset(&args[2], &args[3]) { Some(f) => f, None => return };
            let charset = match std::fs::read_to_string(&args[3]) { Ok(s) => s, Err(e) => { eprintln!("error: {}", e); return; } };
            let sample: String = if args.len() >= 6 {
                args[5..].join(" ")
            } else {
                charset.chars().filter(|c| !c.is_whitespace()).take(40).collect()
            };
            let mut surf = render::Surface::new(1280, 720, [20, 20, 40, 255]);
            surf.fill_rect(40, 500, 1200, 160, [0, 0, 0, 180]);
            font.draw(&mut surf, &sample, 60, 520);
            if let Err(e) = surf.save_tga(&args[4]) { eprintln!("error writing {}: {}", args[4], e); std::process::exit(1); }
            println!("rendered {} chars of sample text -> {}", sample.chars().count(), args[4]);
        }
        "scene" => {
            if args.len() < 7 {
                eprintln!("usage: wa2-native scene <scenario.txt> <font.tga> <charset.txt> <bg.tga> <out.tga>");
                return;
            }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            let font = match load_font_charset(&args[3], &args[4]) { Some(f) => f, None => return };
            let bg = match std::fs::read(&args[5]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("error: could not decode background {}", args[5]); return; }
            };
            // Optional 6th argument: an `.AMP` colour curve, for previewing the
            // sepia/nega flashback looks without running the game.
            let lut = args.get(7).and_then(|p| {
                let d = std::fs::read(p).ok()?;
                eprintln!("tint: {} ({} bytes)", p, d.len());
                Some(d)
            });
            let surf =
                render::render_scene_ex(&bg, &page.text(), &font, 1280, 720, &[], lut.as_deref(), true, None, None);
            if let Err(e) = surf.save_tga(&args[6]) { eprintln!("error writing {}: {}", args[6], e); std::process::exit(1); }
            println!("scene {} (voice={:?} bg={:?} next={:?}): {} lines, {} chars -> {}",
                args[2], page.voice, page.background, page.next, page.lines.len(), page.text().chars().count(), args[6]);
        }
        "scene-map" => {
            // scene-map <scenario.txt> <font.tga> <map.txt> <bg.tga> <out.tga>
            if args.len() < 7 {
                eprintln!("usage: wa2-native scene-map <scenario.txt> <font.tga> <map.txt> <bg.tga> <out.tga>");
                return;
            }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            let atlas = match std::fs::read(&args[3]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("font decode fail"); return; }
            };
            let font = match render::Font::from_map_file(atlas, 24, &args[4]) {
                Ok(f) => f, Err(e) => { eprintln!("map: {}", e); return; }
            };
            let bg = match std::fs::read(&args[5]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("bg fail"); return; }
            };
            let surf = render::render_scene(&bg, &page.text(), &font, 1280, 720);
            if let Err(e) = surf.save_tga(&args[6]) { eprintln!("{}", e); return; }
            println!("scene-map: {} chars -> {}", page.text().chars().count(), args[6]);
        }
        "audio-test" => {
            // audio-test <file.ogg> [seconds]
            if args.len() < 3 {
                eprintln!("usage: wa2-native audio-test <file> [seconds]");
                return;
            }
            let secs: f64 = if args.len() >= 4 { args[3].parse().unwrap_or(5.0) } else { 5.0 };
            let win = match sdl2::Sdl::new("wa2-audio", 320, 240) { Some(w) => w, None => { eprintln!("no SDL/display"); return; } };
            let mixer = match audio::Mixer::new(&win) { Some(m) => m, None => { eprintln!("audio device failed"); return; } };
            eprintln!("playing {} for {}s", args[2], secs);
            mixer.play(&args[2], 1.0, false);
            let t0 = std::time::Instant::now();
            while t0.elapsed().as_secs_f64() < secs {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            println!("audio-test done: driver={} pulled {} samples, queued {} bytes", sdl2::sdl_audio_driver(), audio::mixer::pulled_samples(), audio::mixer::queued_bytes());
        }
        "movie-test" => {
            // movie-test <mv.pak> [--audio <ogg>]
            if args.len() < 3 {
                eprintln!("usage: wa2-native movie-test <mv.pak> [bgm.ogg]");
                return;
            }
            let mut movie = match video::Movie::open(&args[2]) { Some(m) => m, None => { eprintln!("ffmpeg/ffprobe failed or no video"); return; } };
            eprintln!("movie {}x{} @ {:.3}fps", movie.width, movie.height, movie.fps);
            let mut win = match sdl2::Sdl::new("wa2-movie", movie.width as i32 as u32, movie.height) { Some(w) => w, None => { eprintln!("no SDL/display"); return; } };
            let mixer = if args.len() >= 4 { audio::Mixer::new(&win) } else { None };
            if let Some(ref m) = mixer { let _ = m; if let Some(ref mx) = mixer { mx.play(&args[3], 1.0, false); } }
            let mut frame = vec![0u8; (movie.width * movie.height * 4) as usize];
            let frame_delay = std::time::Duration::from_secs_f64(1.0 / movie.fps.max(1.0));
            let mut count = 0u64;
            loop {
                if !movie.read_frame(&mut frame) { eprintln!("EOF"); break; }
                win.present_raw(&frame, movie.width, movie.height);
                count += 1;
                std::thread::sleep(frame_delay);
                match win.poll_input() {
                    sdl2::Input::Quit => { eprintln!("quit"); break; }
                    _ => {}
                }
            }
            eprintln!("played {} frames", count);
        }
        "movie-frame" => {
            // movie-frame <mv.pak> <out.tga>  : decode frame 0 and save as TGA
            if args.len() < 4 {
                eprintln!("usage: wa2-native movie-frame <mv.pak> <out.tga>");
                return;
            }
            let mut movie = match video::Movie::open(&args[2]) { Some(m) => m, None => { eprintln!("open fail"); return; } };
            let want: usize = if args.len() >= 5 { args[4].parse().unwrap_or(0) } else { 0 };
            let mut frame = vec![0u8; (movie.width * movie.height * 4) as usize];
            for i in 0..=want {
                if !movie.read_frame(&mut frame) { eprintln!("no frame {}", i); return; }
            }
            let mut s = render::Surface::new(movie.width, movie.height, [0, 0, 0, 255]);
            for y in 0..movie.height {
                for x in 0..movie.width {
                    let si = ((y * movie.width + x) * 4) as usize;
                    let c = [frame[si], frame[si + 1], frame[si + 2], frame[si + 3]];
                    s.blend_pixel(x as i32, y as i32, c);
                }
            }
            if let Err(e) = s.save_tga(&args[3]) { eprintln!("{}", e); return; }
            println!("saved frame0 {}x{} -> {}", movie.width, movie.height, args[3]);
        }
        "scene-noto" => {
            // scene-noto <scenario.txt> <bg.tga> <out.tga>
            if args.len() < 5 {
                eprintln!("usage: wa2-native scene-noto <scenario.txt> <bg.tga> <out.tga>");
                return;
            }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            let bg = match std::fs::read(&args[3]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("bg fail"); return; }
            };
            let surf = render::render_scene_sysfont(&bg, &page.text(), 1280, 720, render::text::DEFAULT_FONT);
            if let Err(e) = surf.save_tga(&args[4]) { eprintln!("{}", e); return; }
            println!("scene-noto: {} chars -> {}", page.text().chars().count(), args[4]);
        }
        "play-noto" => {
            // play-noto <scenario.txt> <bg.tga> [bgm.ogg]
            if args.len() < 4 {
                eprintln!("usage: wa2-native play-noto <scenario.txt> <bg.tga> [bgm.ogg]");
                return;
            }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            let bg = match std::fs::read(&args[3]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("bg fail"); return; }
            };
            let mut win = match sdl2::Sdl::new("wa2-native", 1280, 720) { Some(w) => w, None => { eprintln!("no SDL/display"); return; } };
            let _mixer = if args.len() >= 5 { audio::Mixer::new(&win).map(|m| { m.play(&args[4], 0.8, true); m }) } else { None };
            eprintln!("play-noto: {} lines (system font). Space/Right/Click=next, Left=back, Esc/q=quit", page.lines.len());
            let mut idx: i32 = 0;
            let n = page.lines.len() as i32;
            loop {
                let surf = render::render_scene_sysfont(&bg, &page.lines[idx.max(0) as usize], 1280, 720, render::text::DEFAULT_FONT);
                win.present(&surf);
                loop {
                    match win.poll_input() {
                        sdl2::Input::Advance => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Back => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Up => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Down => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::RightClick => break,
                        sdl2::Input::Drag { .. } => {}
                        sdl2::Input::None => win.delay(16),
                        sdl2::Input::Choice(_) => win.delay(16),
                        sdl2::Input::Pointer { .. } => win.delay(16),
                    }
                }
            }
        }
        "tga-dump" => {
            if args.len() < 4 {
                eprintln!("usage: wa2-native tga-dump <in.tga> <out.tga>");
                return;
            }
            let img = match std::fs::read(&args[2]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("decode fail"); return; }
            };
            let mut s = render::Surface::new(img.width, img.height, [0, 0, 0, 255]);
            for y in 0..img.height {
                for x in 0..img.width {
                    let si = ((y * img.width + x) * 4) as usize;
                    let c = [img.rgba[si], img.rgba[si + 1], img.rgba[si + 2], img.rgba[si + 3]];
                    s.blend_pixel(x as i32, y as i32, c);
                }
            }
            if let Err(e) = s.save_tga(&args[3]) { eprintln!("{}", e); return; }
            println!("dumped {} -> {} ({}x{})", args[2], args[3], img.width, img.height);
        }
        "fmtinfo" => {
            let mut win = match sdl2::Sdl::new("fmt", 64, 64) { Some(w) => w, None => { eprintln!("no SDL/display"); return; } };
            for c in [0x16362004u32, 0x16762004, 0x16462004, 0x16262004] {
                println!("0x{:08X} = {}", c, win.format_name(c));
            }
        }
        "verify-window" => {
            if args.len() < 5 {
                eprintln!("usage: wa2-native verify-window <font.tga> <charset.txt> <bg.tga>");
                return;
            }
            let font = match load_font_charset(&args[2], &args[3]) { Some(f) => f, None => return };
            let bg = match std::fs::read(&args[4]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i, None => { eprintln!("error bg"); return; }
            };
            let surf = render::render_scene(&bg, "\u{4E00}\u{4E8C}\u{4E09}\u{56DB}\u{4E94} test", &font, 1280, 720);
            let mut win = match sdl2::Sdl::new("wa2-verify", 1280, 720) { Some(w) => w, None => { eprintln!("no SDL/display"); return; } };
            win.present(&surf);
            let rb = win.read_back(1280, 720);
            // compare a sample of pixels
            let mut diff = 0u32; let mut nonblack_rb = 0u32; let mut nonblack_surf = 0u32;
            for i in 0..(1280*720) {
                let si = i*4;
                let sr=surf.pixels[si] as i32; let sg=surf.pixels[si+1] as i32; let sb=surf.pixels[si+2] as i32;
                let rr=rb[si] as i32; let rg=rb[si+1] as i32; let rbl=rb[si+2] as i32;
                if sr+sg+sb > 30 { nonblack_surf+=1; }
                if rr+rg+rbl > 30 { nonblack_rb+=1; }
                if (sr-rr).abs()+(sg-rg).abs()+(sb-rbl).abs() > 30 { diff+=1; }
            }
            println!("surface nonblack px: {}, readback nonblack px: {}, differing px: {}", nonblack_surf, nonblack_rb, diff);
            if nonblack_rb > 1000 { println!("SDL render path OK (readback has content)"); } else { println!("SDL readback EMPTY -> render path problem"); }
        }
        "play" => {
            // wa2-native play <scenario.txt> <font.tga> <charset.txt> <bg.tga>
            if args.len() < 6 {
                eprintln!("usage: wa2-native play <scenario.txt> <font.tga> <charset.txt> <bg.tga>");
                return;
            }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            let font = match load_font_charset(&args[3], &args[4]) { Some(f) => f, None => return };
            let bg = match std::fs::read(&args[5]).ok().and_then(|d| codec::tga::decode(&d).ok()) {
                Some(i) => i,
                None => { eprintln!("error: could not decode background {}", args[5]); return; }
            };
            let mut win = match sdl2::Sdl::new("wa2-native", 1280, 720) {
                Some(w) => w,
                None => { eprintln!("error: SDL2 unavailable / no display. Try DISPLAY=:0"); return; }
            };
            eprintln!("wa2-native play: {} lines. Space/Right/Click=next, Left=back, F11=fullscreen, Esc/q=quit", page.lines.len());
            // optional BGM (6th arg)
            let _mixer = if args.len() >= 7 {
                match audio::Mixer::new(&win) {
                    Some(mx) => { let _ = mx.play(&args[6], 0.8, true); eprintln!("BGM: {}", args[6]); Some(mx) }
                    None => None,
                }
            } else { None };
            let mut idx: i32 = 0;
            let n = page.lines.len() as i32;
            loop {
                let seg = &page.lines[idx.max(0) as usize];
                let text = if idx == 0 {
                    format!("{}{}", seg, "\n\n[Space/Right/Click=next  Esc=quit]")
                } else {
                    seg.clone()
                };
                let surf = render::render_scene(&bg, &text, &font, 1280, 720);
                win.present(&surf);
                // wait for input
                loop {
                    match win.poll_input() {
                        sdl2::Input::Advance => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Back => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Up => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Down => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::RightClick => break,
                        sdl2::Input::Drag { .. } => {}
                        sdl2::Input::Choice(_) => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Pointer { pressed, .. } => { if pressed && idx < n - 1 { idx += 1; break; } win.delay(16); }
                        sdl2::Input::None => { win.delay(16); }
                    }
                }
            }
        }
        "run" => {
            // wa2-native run [start_id]                                  (data tree)
            // wa2-native run <script_dir> <font.tga> <charset.txt> <bg_dir> <start_id> [bgm] [voice_dir]
            let mut win = match sdl2::Sdl::new("wa2-native", 1280, 720) {
                Some(w) => w,
                None => { eprintln!("error: SDL2 unavailable / no display. Try DISPLAY=:0"); return; }
            };
            if args.len() < 7 {
                // Data-tree mode: everything comes from `$WA2_DATA` or ./data.
                let d = match data::Data::resolve() { Some(d) => d, None => return };
                let start_id = args.get(2).cloned().unwrap_or_else(|| "1001".to_string());
                let font = match load_font(&d) { Some(f) => f, None => return };
                eprintln!(
                    "wa2-native run: data={} start={} (Space/Click=next, Left=back, 1-9=choice, Esc/q=quit)",
                    d.describe(), start_id
                );
                let session = Session::from_data(&d, None);
                if let Err(e) = run_game(&mut win, &font, &session, &start_id) {
                    eprintln!("run error: {}", e);
                }
                return;
            }
            // Legacy explicit-path mode (kept for debugging single scenes).
            let script_dir = std::path::PathBuf::from(&args[2]);
            let bg_dir = std::path::PathBuf::from(&args[5]);
            let start_id = args[6].clone();
            let font = match load_font_paths(std::path::Path::new(&args[3]), None, Some(std::path::Path::new(&args[4]))) { Some(f) => f, None => return };
            let bgm_path = args.get(7).cloned();
            let voice_dir = args.get(8).map(std::path::PathBuf::from);
            let session = Session {
                script_dir: script_dir.clone(),
                bg_dir: bg_dir.clone(),
                bg_cn: None,
                sprite_dir: find_sprite_dir(&bg_dir, &script_dir),
                movie_dir: bg_dir.join("..").join("mv"),
                voice_dir,
                bgm: bgm_path,
                bg_index: BgIndex::build(&[&bg_dir]),
            };
            eprintln!("wa2-native run: start={} (Space/Click=next, Left=back, 1-9=choice, Esc/q=quit)", start_id);
            if let Err(e) = run_game(&mut win, &font, &session, &start_id) {
                eprintln!("run error: {}", e);
            }
        }
        "title" => {
            // wa2-native title [start_id]                                (data tree)
            // wa2-native title <script_dir> <font.tga> <charset.txt> <bg_dir> [start_id]
            let mut win = match sdl2::Sdl::new("wa2-native", 1280, 720) {
                Some(w) => w,
                None => { eprintln!("error: SDL2 unavailable / no display. Try DISPLAY=:0"); return; }
            };
            let (font, session, start_id, bg_dir) = if args.len() >= 6 {
                let script_dir = std::path::PathBuf::from(&args[2]);
                let bg_dir = std::path::PathBuf::from(&args[5]);
                let font = match load_font_paths(std::path::Path::new(&args[3]), None, Some(std::path::Path::new(&args[4]))) { Some(f) => f, None => return };
                let start_id = args.get(6).cloned().unwrap_or_else(|| "1001".to_string());
                let session = Session {
                    script_dir: script_dir.clone(),
                    bg_dir: bg_dir.clone(),
                    bg_cn: None,
                    sprite_dir: find_sprite_dir(&bg_dir, &script_dir),
                    movie_dir: bg_dir.join("..").join("mv"),
                    voice_dir: None,
                    bgm: None,
                    bg_index: BgIndex::build(&[&bg_dir]),
                };
                (font, session, start_id, bg_dir)
            } else {
                let d = match data::Data::resolve() { Some(d) => d, None => return };
                let font = match load_font(&d) { Some(f) => f, None => return };
                let start_id = args.get(2).cloned().unwrap_or_else(|| "1001".to_string());
                let bg_dir = d.bg();
                let session = Session::from_data(&d, None);
                eprintln!("wa2-native title: data={}", d.describe());
                (font, session, start_id, bg_dir)
            };
            match run_title(&mut win, &font, &session.bg_dirs(), session.bg_cn.is_some()) {
                TitleAction::Start => {
                    eprintln!("title: starting scene {}", start_id);
                    if let Err(e) = run_game(&mut win, &font, &session, &start_id) {
                        eprintln!("run error: {}", e);
                    }
                }
                TitleAction::Load(snap) => {
                    eprintln!("title: loading scene {}", snap.scene);
                    if let Err(e) = run_game_from(&mut win, &font, &session, snap) {
                        eprintln!("run error: {}", e);
                    }
                }
                TitleAction::Quit => eprintln!("title: quit"),
            }
        }
        "lscr-trace" => {
            // wa2-native lscr-trace <script_dir> <start_id> [choice_seq] [max_events]
            // Headless interpreter trace for verification; choice_seq like "1,2".
            if args.len() < 4 {
                eprintln!("usage: wa2-native lscr-trace <script_dir> <start_id> [choice_seq] [max_events]");
                return;
            }
            let dir = std::path::Path::new(&args[2]);
            let mut id = args[3].clone();
            let seq: Vec<u32> = args
                .get(4)
                .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                .unwrap_or_default();
            let max_ev: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(400);
            let mut seq_i = 0usize;
            let mut scenes = 0;
            loop {
                scenes += 1;
                if scenes > 40 {
                    eprintln!("scene chain limit reached");
                    return;
                }
                let prog = match load_program(dir, &id) {
                    Some(p) => p,
                    None => {
                        eprintln!("scene {} not found", id);
                        return;
                    }
                };
                eprintln!(
                    "=== scene {}: {} ops, {} tokens, entry {:#x} ===",
                    id,
                    prog.ops.len(),
                    prog.tokens.len(),
                    prog.entry
                );
                let mut it = vm::lscr::Interp::new(&prog);
                let mut n = 0usize;
                let mut jumped = false;
                while n < max_ev {
                    n += 1;
                    match it.next_event() {
                        vm::lscr::Event::Text { text, speaker, voice } => {
                            let body = clean_text(&text).replace('\n', " / ");
                            let sp = speaker.unwrap_or_default();
                            let vt = voice.map(|v| format!("{}_{:04}_{:02}", v.scenario, v.line, v.char_id)).unwrap_or_default();
                            println!("  [text] speaker={:?} voice={} | {}", sp, vt, body);
                        }
                        vm::lscr::Event::BgScene { id, variant, frames } =>
                            println!("  [bg  ] id={} var={} frames={}", id, variant, frames),
                        vm::lscr::Event::Overlay { name, slot } => println!("  [ovl ] slot={} {}", slot, name),
                        vm::lscr::Event::WaitFrames(n) => println!("  [wait] {} frames", n),
                        vm::lscr::Event::Sound { opcode, args } =>
                            println!("  [sound] ACT{} {:?}", opcode, args),
                        vm::lscr::Event::Tint { name } => println!("  [tint] {}", if name.is_empty() { "<none>" } else { &name }),
                        vm::lscr::Event::Movie { name } => println!("  [mv  ] {}", name),
                        vm::lscr::Event::Sprite { char_id, series, variant, slot } =>
                            println!("  [spr ] char={} series={} variant={} slot={}", char_id, series, variant, slot),
                        vm::lscr::Event::SpriteExit { char_id, frames } =>
                            println!("  [spr-] char={} frames={}", char_id, frames),
                        vm::lscr::Event::SpriteFocus { char_id } => println!("  [fo-c] {}", char_id),
                        vm::lscr::Event::Choice { options } => {
                            let pick = seq.get(seq_i).copied().unwrap_or(1);
                            seq_i += 1;
                            println!("  [choice] {} options -> picking {}", options.len(), pick);
                            for (i, o) in options.iter().enumerate() {
                                let c = clean_text(o);
                                let c = vm::strip_choice_prefix(&c).unwrap_or(c);
                                println!("           {}. {}", i + 1, c);
                            }
                            it.set_choice_result(pick);
                        }
                        vm::lscr::Event::Wait => {}
                        vm::lscr::Event::Jump { target, kind } => {
                            println!("  [jump] {} -> {}", kind, target);
                            id = target;
                            jumped = true;
                            break;
                        }
                        vm::lscr::Event::End => {
                            println!("  [end ] scene {} ended", id);
                            return;
                        }
                    }
                }
                if !jumped {
                    println!("  [stop] event limit reached in scene {}", id);
                    return;
                }
            }
        }
        "sjis" => {
            if args.len() < 3 { eprintln!("usage: wa2-native sjis <file>"); return; }
            let d = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            println!("{}", codec::sjis::decode(&d));
        }
        "text-info" => {
            if args.len() < 3 { eprintln!("usage: wa2-native text-info <scenario.txt>"); return; }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let page = vm::parse_scenario_text(&scen);
            println!("voice     : {:?}", page.voice);
            println!("speaker   : {:?}", page.speaker);
            println!("background: {:?}", page.background);
            println!("next      : {:?}", page.next);
            println!("lines     : {}", page.lines.len());
            for (i, l) in page.lines.iter().take(12).enumerate() { println!("  [{}] {}", i, l); }
        }
        "steps-info" => {
            if args.len() < 3 { eprintln!("usage: wa2-native steps-info <scenario.txt>"); return; }
            let scen = match std::fs::read(&args[2]) { Ok(d) => d, Err(e) => { eprintln!("error: {}", e); return; } };
            let steps = vm::parse_steps(&scen);
            println!("steps: {}", steps.len());
            for (i, s) in steps.iter().enumerate() {
                match s {
                    vm::Step::Bg(n) => println!("  [{:>3}] bg    {}", i, n),
                    vm::Step::Movie(n) => println!("  [{:>3}] mv    {}", i, n),
                    vm::Step::Say { speaker, text, voice_line } => println!("  [{:>3}] say   {:?} (v={:?}): {}", i, speaker, voice_line, text),
                    vm::Step::Mono(t) => println!("  [{:>3}] mono  {}", i, t),
                    vm::Step::Choice(opts) => {
                        println!("  [{:>3}] choice {} options:", i, opts.len());
                        for (j, o) in opts.iter().enumerate() {
                            println!("         {}. {}", j + 1, o);
                        }
                    }
                    vm::Step::Jump { target, kind } => println!("  [{:>3}] jump  {} -> {}", i, kind, target),
                }
            }
        }
        _ => print_usage(),
    }
}

/// Load a scenario by id from `script_dir` (looks for `<id>.txt`).
fn load_scenario(script_dir: &std::path::Path, id: &str) -> Option<Vec<u8>> {
    let p = script_dir.join(format!("{}.txt", id));
    std::fs::read(&p).ok()
}

/// Try to load a background / UI TGA by name, searching `dirs` in order (the
/// Chinese override directory first, then the shared background directory).
/// Returns None if the file is missing or fails to decode (the caller keeps the
/// previous background).
///
/// WA2 scripts sometimes reference a suffixed variant (e.g. `v100200_bg.tga`)
/// while the archive contains the base name (`v100200.tga`). We try the exact
/// name first, then strip common suffixes.
fn load_background(dirs: &[&std::path::Path], name: &str) -> Option<codec::tga::TgaImage> {
    // Build candidate names: exact, then with suffixes stripped.
    let mut candidates = vec![name.to_string()];
    let lower = name.to_ascii_lowercase();
    for sfx in ["_bg", "_chr", "_chreff", "_ev", "_fg", "_eff"] {
        if lower.ends_with(sfx) {
            candidates.push(name[..name.len() - sfx.len()].to_string());
        }
    }
    // Also try with .tga extension added if missing.
    let base = name.trim_end_matches(".tga").trim_end_matches(".TGA");
    candidates.push(format!("{}.tga", base));
    candidates.push(format!("{}.TGA", base));

    for dir in dirs {
        for cand in &candidates {
            let p = dir.join(cand);
            if let Ok(data) = std::fs::read(&p) {
                if let Ok(img) = codec::tga::decode(&data) {
                    return Some(img);
                }
            }
        }
    }
    None
}

/// Exact voice lookup by the identifiers from ACT137 and ACT138.
struct VoiceIndex {
    map: std::collections::HashMap<(u32, u32, u32), std::path::PathBuf>,
}

impl VoiceIndex {
    fn build(dir: &std::path::Path) -> VoiceIndex {
        let mut map = std::collections::HashMap::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some((stem, ext)) = name.rsplit_once('.') else { continue };
                if !ext.eq_ignore_ascii_case("ogg") { continue; }
                let parts: Vec<_> = stem.split('_').collect();
                if parts.len() != 3 { continue; }
                if let (Ok(scene), Ok(line), Ok(character)) = (parts[0].parse(), parts[1].parse(), parts[2].parse()) {
                    map.insert((scene, line, character), e.path());
                }
            }
        }
        eprintln!("voice: indexed {} clips from {}", map.len(), dir.display());
        VoiceIndex { map }
    }

    fn find(&self, cue: vm::lscr::VoiceCue) -> Option<&std::path::Path> {
        self.map.get(&(cue.scenario, cue.line, cue.char_id)).map(|p| p.as_path())
    }
}

fn stop_voice(mx: &audio::Mixer, voice_stream: &mut Option<usize>) {
    if let Some(slot) = voice_stream.take() {
        mx.stop(slot);
    }
}

/// Start one line's voice. `page_voice` stops whatever was still playing.
fn play_voice(
    mx: &audio::Mixer,
    voices: &VoiceIndex,
    settings: &menu::Settings,
    voice_stream: &mut Option<usize>,
    cue: vm::lscr::VoiceCue,
) {
    if settings.page_voice == 1 {
        stop_voice(mx, voice_stream);
    }
    let muted = menu::char_row_of(cue.char_id)
        .map(|row| settings.char_voice[row] == 0)
        .unwrap_or(false);
    if muted {
        return;
    }
    if let Some(vf) = voices.find(cue) {
        eprintln!("  voice: {}", vf.display());
        let slot = mx.play_cat(&vf.to_string_lossy(), 1.0, false, crate::audio::mixer::CAT_VOICE);
        if slot != usize::MAX {
            *voice_stream = Some(slot);
        }
    } else {
        eprintln!(
            "  voice: MISSING {}_{:04}_{:02}.OGG",
            cue.scenario, cue.line, cue.char_id
        );
    }
}

/// Load one of the game's `.AMP` colour curves by name (case-insensitive).
fn load_tint(dirs: &[&std::path::Path], name: &str) -> Option<Vec<u8>> {
    let want = name.to_ascii_lowercase();
    for dir in dirs {
        // exact first, then a case-insensitive scan (sepia.AMP vs sepia.amp)
        if let Ok(d) = std::fs::read(dir.join(name)) {
            return Some(d);
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Ok(n) = e.file_name().into_string() {
                    if n.to_ascii_lowercase() == want {
                        if let Ok(d) = std::fs::read(e.path()) {
                            return Some(d);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Build a solid-color fallback background image.
fn solid_bg(w: u32, h: u32, color: [u8; 3]) -> codec::tga::TgaImage {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
    }
    codec::tga::TgaImage { width: w, height: h, rgba }
}

/// Standing-character stage slots. K16's first value is the slot: 0/1/2 are
/// the usual left/centre/right, 3/4 stand further out, and 7–10 are the back
/// row used in group shots. Clamping everything above 2 onto the right slot
/// stacked people who should have been standing apart.
const SPRITE_SLOTS: usize = 11;
/// Horizontal centre of each slot, in 1280-wide screen coordinates.
const SPRITE_ANCHORS: [i32; SPRITE_SLOTS] = [
    320, 640, 960, // 0 left, 1 centre, 2 right
    140, 1140, // 3 far left, 4 far right
    640, 640, // 5, 6 (unused by the scripts)
    220, 500, 780, 1060, // 7–10 back row
];
/// Paint the back row and the outer pair before the front three, so a group
/// shot doesn't hide the people standing behind.
const SPRITE_DRAW_ORDER: [usize; SPRITE_SLOTS] = [7, 8, 9, 10, 3, 4, 5, 6, 0, 1, 2];

/// Background file index for the ACT146/147 ids.
///
/// Files are named `b{id:04}{variant:02}.tga` (id 1004 variant 0 -> b100400.tga,
/// id 1014 variant 10 -> b101410.tga). A few ids only ship one variant
/// (b104402.tga with no b104400.tga), so each id keeps a fallback entry, with
/// the `00` variant preferred.
struct BgIndex {
    by_name: std::collections::HashMap<String, String>,
    by_id: std::collections::HashMap<u32, String>,
}

impl BgIndex {
    fn build(dirs: &[&std::path::Path]) -> BgIndex {
        use std::collections::hash_map::Entry;
        let mut by_name = std::collections::HashMap::new();
        let mut by_id = std::collections::HashMap::new();
        for dir in dirs {
            let rd = match std::fs::read_dir(dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for e in rd.flatten() {
                let name = match e.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let lower = name.to_ascii_lowercase();
                if !lower.starts_with('b') || !lower.ends_with(".tga") {
                    continue;
                }
                let digits = lower[1..lower.len() - 4].to_string();
                if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let id: u32 = digits[..4].parse().unwrap_or(0);
                by_name.entry(lower).or_insert_with(|| name.clone());
                match by_id.entry(id) {
                    Entry::Vacant(v) => {
                        v.insert(name.clone());
                    }
                    Entry::Occupied(mut o) => {
                        if digits.ends_with("00") {
                            o.insert(name.clone());
                        }
                    }
                }
            }
        }
        BgIndex { by_name, by_id }
    }

    /// `b{id:04}{variant:02}.tga`, else any file of that id.
    fn resolve(&self, id: i64, variant: u32) -> Option<String> {
        if id <= 0 {
            return None;
        }
        let exact = format!("b{:04}{:02}.tga", id, variant);
        if let Some(n) = self.by_name.get(&exact) {
            return Some(n.clone());
        }
        self.by_id.get(&(id as u32)).cloned()
    }
}

/// Where every kind of media comes from for one play session.
///
/// Built either from the loose-file data tree (`Session::from_data`) or from
/// explicit CLI paths (legacy debugging form). Everything the engine loads at
/// runtime goes through here - no `.pak` archive is opened.
struct Session {
    script_dir: std::path::PathBuf,
    bg_dir: std::path::PathBuf,
    /// Chinese-patch UI overrides - the 31 images shipped loose in the game's
    /// `grp/` directory, which win over the same-named files inside `grp.pak`.
    /// Consulted before `bg_dir`; `None` in Japanese mode.
    bg_cn: Option<std::path::PathBuf>,
    /// Loose `*.tga` character sprites, or `None` to disable sprites.
    sprite_dir: Option<std::path::PathBuf>,
    /// Movies as plain `*.wmv` / `*.asf` files.
    movie_dir: std::path::PathBuf,
    voice_dir: Option<std::path::PathBuf>,
    bgm: Option<String>,
    /// ACT146/147 background id -> file index.
    bg_index: BgIndex,
}

impl Session {
    fn from_data(d: &data::Data, bgm: Option<String>) -> Session {
        Session {
            script_dir: d.script(),
            bg_dir: d.bg(),
            bg_cn: if d.cn { Some(d.root.join("bg_cn")) } else { None },
            sprite_dir: Some(d.sprite()),
            movie_dir: d.movie(),
            voice_dir: Some(d.voice()),
            bgm,
            bg_index: BgIndex::build(&[&d.bg()]),
        }
    }

    /// Directories searched for images, in priority order.
    fn bg_dirs(&self) -> Vec<&std::path::Path> {
        let mut v = Vec::new();
        if let Some(cn) = self.bg_cn.as_deref() {
            v.push(cn);
        }
        v.push(self.bg_dir.as_path());
        v
    }
}

/// Full game loop: play scenes starting from `start_id`, following CATCH jumps
/// and presenting choices to the player.
/// Play from the start of a scene.
pub fn run_game(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    start_id: &str,
) -> Result<(), String> {
    // Loop so that loading a save slot from the command bar restarts the run
    // at the loaded position instead of ending the game.
    let mut resume: Option<save::SaveState> = None;
    loop {
        match run_game_at(win, font, s, resume, start_id)? {
            None => return Ok(()),
            Some(snap) => resume = Some(snap),
        }
    }
}

/// Resume from a save slot.
pub fn run_game_from(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    snap: save::SaveState,
) -> Result<(), String> {
    let mut resume = Some(snap);
    let scene = resume.as_ref().unwrap().scene.clone();
    loop {
        match run_game_at(win, font, s, resume, &scene)? {
            None => return Ok(()),
            Some(st) => resume = Some(st),
        }
    }
}

/// `resume` rebuilds the state of a save slot before the first scene runs.
fn run_game_at(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    resume: Option<save::SaveState>,
    start_id: &str,
) -> Result<Option<save::SaveState>, String> {
    let mut resume = resume;
    let script_dir = s.script_dir.as_path();
    let bg_dir = s.bg_dir.as_path();
    let voice_dir = s.voice_dir.as_deref();
    let bgm_path = s.bgm.as_deref();
    const W: u32 = 1280;
    const H: u32 = 720;
    // Function buttons (sys_00010) and the message-window frame (sys_00000).
    let bar = cmdbar::CmdBar::load(&s.bg_dirs());
    let msg_plate = load_background(&s.bg_dirs(), "sys_00001.tga");
    let msg_frame = load_background(&s.bg_dirs(), "sys_00000.tga");
    let mut cmd_auto = false;
    let mut cmd_skip = false;
    // Lines already shown, for the 履历 (backlog) screen.
    let mut backlog: Vec<String> = Vec::new();
    let mut current_id = start_id.to_string();
    // Black until the script sets one: WA2 opens on a black screen (the IC
    // prologue's "sepia.AMP" is a color-filter LUT, not a background image).
    let mut bg = solid_bg(W, H, [0, 0, 0]);

    // Start BGM if requested (and keep mixer for voice playback).
    let mixer = if bgm_path.is_some() || voice_dir.is_some() || s.script_dir.parent().map(|p| p.join("bgm").is_dir() || p.join("se").is_dir()).unwrap_or(false) {
        match audio::Mixer::new(win) {
            Some(mx) => {
                if let Some(bgm) = bgm_path {
                    let _ = mx.play(bgm, 0.6, true);
                    eprintln!("BGM: {}", bgm);
                }
                Some(mx)
            }
            None => None,
        }
    } else {
        None
    };

    // WA2_AUTO       : advance text *and* pick choices automatically (unattended runs)
    // WA2_AUTO_TEXT  : advance text only, stop at choices (interactive verification)
    let auto_pick = std::env::var_os("WA2_AUTO").is_some();
    let auto_text = auto_pick || std::env::var_os("WA2_AUTO_TEXT").is_some();

    // Voice files are indexed once: the directory holds ~39k entries and the
    // old code scanned it for every single line.
    let voices = voice_dir.map(VoiceIndex::build);
    // Player settings (option screen / wa2.ini).
    let mut settings = menu::Settings::load();
    menu::apply(&settings, mixer.as_ref());

    let mut bgm_track: Option<(i32, usize)> = None;
    let mut fading_bgm: Option<usize> = None;
    let mut voice_stream: Option<usize> = None;
    // Voice of the line on screen while skip is running. Skip does not play
    // it; turning skip off does, so the clip matches the text.
    let mut skipped_voice: Option<vm::lscr::VoiceCue> = None;
    let mut se_channels = std::collections::HashMap::<i32, usize>::new();
    // SE channels that are fading out (ACT166) but still audible. Without this
    // the slot leaves `se_channels` immediately, so a new SE on the same channel
    // plays *on top of* the fading one - the BGM path keeps `fading_bgm` for
    // exactly this reason.
    let mut fading_se = std::collections::HashMap::<i32, usize>::new();

    let mut cur_surf = render::render_scene(&bg, "", font, W, H);
    let mut bank = s.sprite_dir.as_ref().filter(|p| p.is_dir()).map(|dir| {
        eprintln!("sprites: {}", dir.display());
        SpriteBank::new(dir.clone())
    });
    if bank.is_none() {
        eprintln!("note: no sprite directory - character sprites disabled");
    }
    // Standing sprites live in numbered stage slots (K16's first value): slot 0
    // left, 1 centre, 2 right. Slots are reused as characters move around.
    let mut slots: [Option<ActiveSprite>; SPRITE_SLOTS] = Default::default();
    // Active `.AMP` colour curve (sepia flashbacks, negative, ...).
    let mut tint: Option<Vec<u8>> = None;
    let mut focus: Option<u32> = None;
    // 「ウィンドウ一時消去」: when false, the dialog box and its text are hidden
    // (the scene art stays). Toggled by the bottom command bar.
    let mut window = true;
    // Only these three are extra bookkeeping for save/load. The dialog box is
    // not state - `cur_surf` holds the composited pixels but not the string -
    // and the decoded background/tint cannot be stored, so their ids and names
    // are kept next to them and re-resolved when a save is loaded.
    let mut last_text = String::new();
    let mut last_speaker: Option<String> = None;
    let mut tint_name: Option<String> = None;
    let mut bg_id: Option<(i64, u32)> = None;
    // ---- resume from a save slot ----------------------------------------
    if let Some(snap) = resume.as_ref() {
        current_id = snap.scene.clone();
        bg_id = snap.bg;
        if let Some((id, variant)) = snap.bg {
            if let Some(name) = s.bg_index.resolve(id, variant) {
                if let Some(img) = load_background(&s.bg_dirs(), &name) {
                    bg = img;
                }
            }
        }
        tint_name = snap.tint.clone();
        if let Some(name) = snap.tint.as_deref() {
            tint = load_tint(&s.bg_dirs(), name);
        }
        for (i, entry) in snap.slots.iter().enumerate() {
            let (char_id, name) = match entry {
                Some(e) => e,
                None => continue,
            };
            if let Some(img) = bank.as_mut().and_then(|b| b.get(name)).cloned() {
                slots[i] = Some(ActiveSprite { char_id: *char_id, name: name.clone(), img });
            }
        }
        focus = snap.focus;
        let (saved_name, saved_body) = split_saved_line(&snap.text);
        last_text = saved_body;
        last_speaker = snap
            .vm
            .speaker
            .as_ref()
            .map(|(n, _)| n.clone())
            .or(saved_name);
        // BGM comes back by id: the mixer slot it was playing on is gone.
        if let (Some(mx), Some(id)) = (mixer.as_ref(), snap.bgm) {
            let dir = script_dir.parent().unwrap_or(std::path::Path::new(".")).join("bgm");
            let stem = format!("BGM_{:03}", id);
            let path = [format!("{}.OGG", stem), format!("{}_A.OGG", stem)]
                .into_iter()
                .map(|n| dir.join(n))
                .find(|p| p.is_file());
            if let Some(path) = path {
                let slot = mx.play_cat(
                    &path.to_string_lossy(),
                    1.0,
                    true,
                    crate::audio::mixer::CAT_BGM,
                );
                if slot != usize::MAX {
                    bgm_track = Some((id, slot));
                }
            }
        }
        cur_surf = draw_scene(&bg, &last_text, last_speaker.as_deref(), font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
        win.present(&cur_surf);
        eprintln!("load: scene {} at pc {}", current_id, snap.vm.pc);
        // The save was taken while the engine waited for input, and `pc` is
        // already past that wait, so the saved line has to be shown and waited
        // on here - otherwise the interpreter would run straight into the next
        // line and the player would never see where they left off.
        if auto_text {
            win.delay(300);
        } else {
            match wait_input(win) {
                sdl2::Input::Quit => return Ok(None),
                _ => {}
            }
        }
    }
    loop {
        let prog = match load_program(script_dir, &current_id) {
            Some(p) => p,
            None => {
                eprintln!("scene {} not found (.bnr/.txt)", current_id);
                return Ok(None);
            }
        };
        eprintln!(
            "scene {}: {} ops, {} tokens, entry {:#x}",
            current_id,
            prog.ops.len(),
            prog.tokens.len(),
            prog.entry
        );
        let mut it = vm::lscr::Interp::new(&prog);
        // The saved position belongs to the first scene only; a later scene
        // change means the player moved on, so it starts from its entry point.
        if let Some(snap) = resume.take() {
            it.restore(snap.vm);
        }
        loop {
            match it.next_event() {
                vm::lscr::Event::Text { text, speaker, voice } => {
                    let body = clean_text(&text);
                    let muted = voice
                        .and_then(|c| menu::char_row_of(c.char_id))
                        .map(|row| settings.char_voice[row] == 0)
                        .unwrap_or(false);
                    let cue = voice.filter(|_| !muted);
                    if cmd_skip {
                        // Starting a clip per skipped line piles ffmpeg jobs up
                        // across a scene change, and the one still audible is
                        // no longer the line on screen.
                        skipped_voice = cue;
                        if let Some(mx) = mixer.as_ref() {
                            stop_voice(mx, &mut voice_stream);
                        }
                    } else if let (Some(mx), Some(vi), Some(cue)) = (mixer.as_ref(), voices.as_ref(), cue) {
                        skipped_voice = None;
                        play_voice(mx, vi, &settings, &mut voice_stream, cue);
                    } else if settings.page_voice == 1 {
                        skipped_voice = None;
                        if let Some(mx) = mixer.as_ref() {
                            stop_voice(mx, &mut voice_stream);
                        }
                    }
                    cur_surf = draw_scene(&bg, &body, speaker.as_deref(), font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
                    win.present(&cur_surf);
                    // Remember the current line for save (last_text) and backlog.
                    last_speaker = speaker.clone();
                    last_text = body.clone();
                    let logged = match &speaker {
                        Some(sp) => format!("{}：{}", sp, body),
                        None => body.clone(),
                    };
                    backlog.push(logged);
                    if backlog.len() > 200 { backlog.remove(0); }
                }
                vm::lscr::Event::Sprite { char_id, series, variant, slot } => {
                    if let Some(bank) = bank.as_mut() {
                        if let Some(name) = bank.resolve(char_id, series, variant) {
                            if let Some(img) = bank.get(&name) {
                                let img = img.clone();
                                let idx = slot as usize;
                                // One character occupies one slot. Moving them
                                // (雪菜 from 2 to 1, and so on) has to clear
                                // the place they just left.
                                for s in slots.iter_mut() {
                                    if s.as_ref().is_some_and(|a| a.char_id == char_id) {
                                        *s = None;
                                    }
                                }
                                if let Some(dst) = slots.get_mut(idx) {
                                    eprintln!("  sprite: {} (char {}, slot {})", name, char_id, idx);
                                    *dst = Some(ActiveSprite { char_id, name, img });
                                } else {
                                    eprintln!("  sprite: slot {} out of range", idx);
                                }
                            }
                        } else {
                            eprintln!("  sprite: char {} series {:03} variant {:03} not found", char_id, series, variant);
                        }
                    }
                }
                vm::lscr::Event::SpriteExit { char_id, frames } => {
                    let before = slots.iter().flatten().filter(|a| a.char_id == char_id).count();
                    for s in slots.iter_mut() {
                        if s.as_ref().is_some_and(|a| a.char_id == char_id) {
                            *s = None;
                        }
                    }
                    eprintln!("  sprite exit: char {} ({}f, removed {})", char_id, frames, before);
                }
                vm::lscr::Event::SpriteFocus { char_id } => {
                    focus = Some(char_id);
                }
                vm::lscr::Event::Tint { name } => {
                    // Colour filter only - the background stays as it is.
                    tint_name = if name.is_empty() { None } else { Some(name.clone()) };
                    tint = if name.is_empty() {
                        eprintln!("  tint: <none>");
                        None
                    } else {
                        let dirs = s.bg_dirs();
                        match load_tint(&dirs, &name) {
                            Some(lut) => {
                                eprintln!("  tint: {} ({} bytes)", name, lut.len());
                                Some(lut)
                            }
                            None => {
                                eprintln!("  tint: {} NOT FOUND", name);
                                None
                            }
                        }
                    };
                    cur_surf = draw_scene(&bg, "", None, font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
                    win.present(&cur_surf);
                }
                vm::lscr::Event::BgScene { id, variant, frames } => {
                    // ACT146/147: the real background op. 0 and -2 mean black.
                    bg_id = if id == 0 || id == -2 { None } else { Some((id, variant)) };
                    if id == 0 || id == -2 {
                        eprintln!("  bg: <black> (id {})", id);
                        bg = solid_bg(W, H, [0, 0, 0]);
                    } else if let Some(name) = s.bg_index.resolve(id, variant) {
                        if let Some(img) = load_background(&s.bg_dirs(), &name) {
                            eprintln!("  bg: {} (id {} var {} fade {}f)", name, id, variant, frames);
                            bg = img;
                        }
                    } else {
                        eprintln!("  bg: id {} var {} has no file, keeping current", id, variant);
                    }
                    cur_surf = draw_scene(&bg, "", None, font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
                    win.present(&cur_surf);
                }
                vm::lscr::Event::Overlay { name, slot } => {
                    // Items / effect plates / composite-CG layers. Layered
                    // rendering is not implemented yet, so this only records it.
                    eprintln!("  overlay: slot {} {}", slot, name);
                }
                vm::lscr::Event::WaitFrames(n) => {
                    // Scripted pause (camera holds, fades). Skip mode does not
                    // sit through it: a new scene often opens with one of
                    // these, and the button would ignore clicks until it ended.
                    if !cmd_skip {
                        let total = std::time::Duration::from_secs_f64(n as f64 / 60.0);
                        let t0 = std::time::Instant::now();
                        while t0.elapsed() < total && !cmd_skip {
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(None),
                                sdl2::Input::Pointer { x, y, pressed: true } => {
                                    let (sx, sy) = win.surface_pos(x, y, W, H);
                                    if bar.hit(sx, sy) == Some(cmdbar::Cmd::Skip) {
                                        cmd_skip = true;
                                    }
                                }
                                _ => {}
                            }
                            win.delay(16);
                        }
                    }
                }
                vm::lscr::Event::Sound { opcode, args } => {
                    let arg = |i: usize| args.get(i).copied().unwrap_or(0);
                    let root = s.script_dir.parent().unwrap_or(std::path::Path::new("."));
                    let volume = |v: i32| v.clamp(0, 255) as f32 / 255.0;
                    if let Some(mx) = mixer.as_ref() {
                        match opcode {
                            158 if args.len() >= 4 => {
                                let id = arg(0);
                                if id < 0 {
                                    if let Some((_, slot)) = bgm_track.take() { mx.fade(slot, 0.0, arg(1), true); }
                                } else if let Some((_, slot)) = bgm_track.filter(|(old, _)| *old == id) {
                                    mx.fade(slot, volume(arg(3)), arg(1), false);
                                } else {
                                    if let Some(slot) = fading_bgm.take() { mx.stop(slot); }
                                    if let Some((_, slot)) = bgm_track.take() {
                                        mx.fade(slot, 0.0, if arg(1) == -2 { 15 } else { arg(1) }, true);
                                        fading_bgm = Some(slot);
                                    }
                                    let dir = root.join("bgm");
                                    let stem = format!("BGM_{:03}", id);
                                    let path = [format!("{}.OGG", stem), format!("{}_{}.OGG", stem, if arg(4) != 0 { "B" } else { "A" })]
                                        .into_iter().map(|name| dir.join(name)).find(|p| p.is_file());
                                    if let Some(path) = path {
                                        let slot = mx.play_cat(&path.to_string_lossy(), if arg(1) > 0 { 0.0 } else { volume(arg(3)) }, arg(2) != 0, crate::audio::mixer::CAT_BGM);
                                        if slot != usize::MAX {
                                            mx.fade(slot, volume(arg(3)), arg(1), false);
                                            bgm_track = Some((id, slot));
                                            eprintln!("  BGM: ACT158 track {} -> {}", id, path.display());
                                        }
                                    } else { eprintln!("  BGM: missing track {}", id); }
                                }
                            }
                            159 => { if let Some((_, slot)) = bgm_track.take() { mx.fade(slot, 0.0, arg(0), true); } }
                            161 => { if let Some((_, slot)) = bgm_track { mx.fade(slot, volume(arg(0)), arg(1), false); } }
                            165 if arg(5) == 2 => {
                                // Original 0x40f2d0 updates the existing decoder in mode 2.
                                if let Some(&slot) = se_channels.get(&arg(0)) {
                                    mx.fade(slot, volume(arg(4)), arg(2), false);
                                }
                            }
                            164 | 165 => {
                                let (channel, id, frames, looping, vol) = if opcode == 164 {
                                    (None, arg(0), 0, false, arg(1))
                                } else { (Some(arg(0)), arg(1), arg(2), arg(3) == 1, arg(4)) };
                                if let Some(ch) = channel {
                                    if let Some(slot) = se_channels.remove(&ch) { mx.stop(slot); }
                                    // The channel may still be fading out from an ACT166.
                                    if let Some(slot) = fading_se.remove(&ch) { mx.stop(slot); }
                                }
                                let dir = root.join("se");
                                let stem = format!("SE_{:04}", id);
                                let path = ["WAV", "OGG", "wav", "ogg"].into_iter()
                                    .map(|ext| dir.join(format!("{}.{}", stem, ext))).find(|p| p.is_file());
                                if let Some(path) = path {
                                    let slot = mx.play_cat(&path.to_string_lossy(), if frames > 0 { 0.0 } else { volume(vol) }, looping, crate::audio::mixer::CAT_SE);
                                    if slot != usize::MAX {
                                        mx.fade(slot, volume(vol), frames, false);
                                        if let Some(ch) = channel { se_channels.insert(ch, slot); }
                                        eprintln!("  SE: ACT{} ch={:?} id={} -> {}", opcode, channel, id, path.display());
                                    }
                                } else { eprintln!("  SE: missing {}", id); }
                            }
                            166 => {
                                if let Some(slot) = se_channels.remove(&arg(0)) {
                                    mx.fade(slot, 0.0, arg(1), true);
                                    // Keep tracking it until the fade finishes so a
                                    // restart on this channel can stop it first.
                                    fading_se.insert(arg(0), slot);
                                }
                            }
                            167 => { if let Some(&slot) = se_channels.get(&arg(0)) { mx.fade(slot, volume(arg(1)), arg(2), false); } }
                            162 | 163 | 169 => {
                                let slot = if opcode == 169 { se_channels.get(&arg(0)).copied() }
                                    else { bgm_track.map(|(_, slot)| slot).or(fading_bgm) };
                                if let Some(slot) = slot {
                                    while !cmd_skip && if opcode == 163 { !mx.finished(slot) } else { !mx.settled(slot) } {
                                        match win.poll_input() {
                                            sdl2::Input::Quit => return Ok(None),
                                            sdl2::Input::Pointer { x, y, pressed: true } => {
                                                let (sx, sy) = win.surface_pos(x, y, W, H);
                                                if bar.hit(sx, sy) == Some(cmdbar::Cmd::Skip) {
                                                    cmd_skip = true;
                                                }
                                            }
                                            _ => {}
                                        }
                                        win.delay(16);
                                    }
                                }
                            }
                            168 => {
                                if let Some(&slot) = se_channels.get(&arg(0)) {
                                    while !cmd_skip && !mx.finished(slot) {
                                        match win.poll_input() {
                                            sdl2::Input::Quit => return Ok(None),
                                            sdl2::Input::Pointer { x, y, pressed: true } => {
                                                let (sx, sy) = win.surface_pos(x, y, W, H);
                                                if bar.hit(sx, sy) == Some(cmdbar::Cmd::Skip) {
                                                    cmd_skip = true;
                                                }
                                            }
                                            _ => {}
                                        }
                                        win.delay(16);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                vm::lscr::Event::Movie { name } => {
                    eprintln!("  movie: {}", name);
                    let _ = play_movie(win, &s.movie_dir, &name);
                    win.present(&cur_surf);
                }
                vm::lscr::Event::Choice { options } => {
                    let opts: Vec<String> = options
                        .iter()
                        .map(|o| {
                            let c = clean_text(o);
                            vm::strip_choice_prefix(&c).unwrap_or(c)
                        })
                        .collect();
                    let mut body = String::from("\u{2014}\u{2014} \u{9009}\u{62e9} \u{2014}\u{2014}\n");
                    for (i, o) in opts.iter().enumerate() {
                        body.push_str(&format!("{}. {}\n", i + 1, o));
                    }
                    // Option i sits on text line i+1 (line 0 is the header), so its
                    // screen band is known and can be used for pointer hit testing.
                    // Render the plain frame once and copy it per frame - a full
                    // re-render is far too slow to repeat while the mouse moves.
                    let line_h = if msg_plate.is_some() {
                        render::DIALOG_LINE
                    } else {
                        font.cell() as i32 + 4
                    };
                    // Same origin render_scene_ex uses once the game window is up.
                    let text_y = if msg_plate.is_some() {
                        render::DIALOG_TEXT_Y
                    } else {
                        let box_h = (H / 4).min(160).max(120);
                        (H - box_h - 44) as i32 + 18
                    };
                    let opt_top = |i: usize| text_y + (i as i32 + 1) * line_h;
                    let base = draw_scene(&bg, &body, None, font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
                    let mut hover: Option<usize> = None;
                    let mut bar_hover: Option<cmdbar::Cmd> = None;
                    cur_surf = draw_scene(&bg, &body, None, font, &slots, focus, W, H, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref());
                    win.present(&cur_surf);
                    eprintln!(
                        "  choice: presenting {} options (click one, or press 1-{})",
                        opts.len(),
                        opts.len()
                    );
                    let chosen = if auto_pick {
                        win.delay(300);
                        1u32
                    } else {
                        loop {
                            let mut frame = render::Surface {
                                width: base.width,
                                height: base.height,
                                pixels: base.pixels.clone(),
                            };
                            if let Some(i) = hover {
                                let y = opt_top(i) - 6;
                                let (hx, hw) = if msg_plate.is_some() {
                                    (
                                        render::DIALOG_TEXT_X - 16,
                                        (render::DIALOG_TEXT_RIGHT - render::DIALOG_TEXT_X + 32) as u32,
                                    )
                                } else {
                                    (40, W - 80)
                                };
                                frame.fill_rect(hx, y, hw, (line_h + 4) as u32, [130, 190, 255, 70]);
                            }
                            let active = cmdbar::Active { auto: cmd_auto, skip: cmd_skip, hide: !window };
                            bar.draw(&mut frame, bar_hover, active);
                            win.present(&frame);
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(None),
                                sdl2::Input::Choice(n) if (n as usize) <= opts.len() => {
                                    break n as u32
                                }
                                sdl2::Input::Pointer { x, y, pressed } => {
                                    let (sx, sy) = win.surface_pos(x, y, W, H);
                                    // Command bar takes priority over the choices.
                                    if let Some(cmd) = bar.hit(sx, sy) {
                                        if pressed {
                                            match cmd {
                                                cmdbar::Cmd::Auto => cmd_auto = !cmd_auto,
                                                cmdbar::Cmd::Skip => {
                                                    cmd_skip = !cmd_skip;
                                                    if let Some(mx) = mixer.as_ref() {
                                                        if cmd_skip {
                                                            stop_voice(mx, &mut voice_stream);
                                                        } else if let (Some(cue), Some(vi)) = (skipped_voice.take(), voices.as_ref()) {
                                                            play_voice(mx, vi, &settings, &mut voice_stream, cue);
                                                        }
                                                    }
                                                },
                                                cmdbar::Cmd::Hide => {
                                                    window = !window;
                                                    cur_surf = draw_scene(
                                                        &bg,
                                                        &last_text,
                                                        last_speaker.as_deref(),
                                                        font,
                                                        &slots,
                                                        focus,
                                                        W,
                                                        H,
                                                        tint.as_deref(),
                                                        window,
                                                        msg_plate.as_ref(), msg_frame.as_ref(),
                                                    );
                                                }
                                                other => match handle_cmd(
                                                    other,
                                                    win,
                                                    font,
                                                    s,
                                                    &mut settings,
                                                    mixer.as_ref(),
                                                    &it,
                                                    &current_id,
                                                    bg_id,
                                                    &tint_name,
                                                    &slots,
                                                    focus,
                                                    &last_text,
                                                    bgm_track.map(|(id, _)| id),
                                                    &cur_surf,
                                                    &backlog,
                                                ) {
                                                    CmdOutcome::Loaded(st) => return Ok(Some(st)),
                                                    CmdOutcome::Quit => return Ok(None),
                                                    CmdOutcome::Stay => {}
                                                },
                                            }
                                        } else {
                                            bar_hover = Some(cmd);
                                        }
                                        win.delay(16);
                                    } else {
                                        let hit = (0..opts.len()).find(|i| {
                                            sy >= opt_top(*i) - 6 && sy < opt_top(*i) + line_h
                                        });
                                        hover = hit;
                                        bar_hover = None;
                                        if pressed {
                                            if let Some(i) = hit {
                                                break (i + 1) as u32;
                                            }
                                        }
                                        win.delay(16);
                                    }
                                }
                                _ => win.delay(16),
                            }
                        }
                    };
                    eprintln!("  choice: {} / {}", chosen, opts.len());
                    it.set_choice_result(chosen);
                }

                vm::lscr::Event::Wait => {
                    // Always draw the command bar, even in auto-advance modes, so
                    // the player can toggle auto/skip off or open save/load/config.
                    let mut cmd_hover: Option<cmdbar::Cmd> = None;
                    let mut dwell: u32 = 0;
                    'wait: loop {
                            let mut frame = render::Surface {
                                width: cur_surf.width,
                                height: cur_surf.height,
                                pixels: cur_surf.pixels.clone(),
                            };
                            let active = cmdbar::Active {
                                auto: cmd_auto,
                                skip: cmd_skip,
                                hide: !window,
                            };
                            bar.draw(&mut frame, cmd_hover, active);
                            win.present(&frame);
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(None),
                                sdl2::Input::RightClick => {
                                    match settings.r_click {
                                        1 => {
                                            let dirs = s.bg_dirs();
                                            let mut st = menu::Settings::load();
                                            menu::run(win, &dirs, &mut st, mixer.as_ref());
                                            settings = menu::Settings::load();
                                            menu::apply(&settings, mixer.as_ref());
                                        }
                                        2 => {
                                            let st = make_save(
                                                &it,
                                                &current_id,
                                                bg_id,
                                                tint_name.clone(),
                                                &slots,
                                                focus,
                                                &last_text,
                                                bgm_track.map(|(id, _)| id),
                                            );
                                            saveload::run(
                                                win,
                                                &s.bg_dirs(),
                                                saveload::Mode::Save,
                                                Some(st),
                                                Some(&cur_surf),
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                                sdl2::Input::Pointer { x, y, pressed } => {
                                    let (sx, sy) = win.surface_pos(x, y, W, H);
                                    match bar.hit(sx, sy) {
                                        Some(cmd) if pressed => match cmd {
                                            cmdbar::Cmd::Auto => cmd_auto = !cmd_auto,
                                            cmdbar::Cmd::Skip => {
                                                cmd_skip = !cmd_skip;
                                                if let Some(mx) = mixer.as_ref() {
                                                    if cmd_skip {
                                                        stop_voice(mx, &mut voice_stream);
                                                    } else if let (Some(cue), Some(vi)) = (skipped_voice.take(), voices.as_ref()) {
                                                        play_voice(mx, vi, &settings, &mut voice_stream, cue);
                                                    }
                                                }
                                            },
                                            cmdbar::Cmd::Hide => {
                                                window = !window;
                                                cur_surf = draw_scene(
                                                    &bg,
                                                    &last_text,
                                                    last_speaker.as_deref(),
                                                    font,
                                                    &slots,
                                                    focus,
                                                    W,
                                                    H,
                                                    tint.as_deref(),
                                                    window,
                                                    msg_plate.as_ref(), msg_frame.as_ref(),
                                                );
                                            }
                                            other => match handle_cmd(
                                                other,
                                                win,
                                                font,
                                                s,
                                                &mut settings,
                                                mixer.as_ref(),
                                                &it,
                                                &current_id,
                                                bg_id,
                                                &tint_name,
                                                &slots,
                                                focus,
                                                &last_text,
                                                bgm_track.map(|(id, _)| id),
                                                &cur_surf,
                                                &backlog,
                                            ) {
                                                CmdOutcome::Loaded(st) => return Ok(Some(st)),
                                                CmdOutcome::Quit => return Ok(None),
                                                CmdOutcome::Stay => {}
                                            },
                                        },
                                        Some(c) => cmd_hover = Some(c),
                                        None => {
                                            cmd_hover = None;
                                            if pressed {
                                                break 'wait;
                                            }
                                        }
                                    }
                                },
                                sdl2::Input::Advance => break 'wait,
                                // No new event: keep the hover caption where it is.
                                sdl2::Input::None => {}
                                _ => cmd_hover = None,
                            }
                            if auto_text || cmd_auto || cmd_skip {
                                dwell += 16;
                                // Skip ignores the slider. Auto holds for the
                                // menu's 「自动模式速度」, and a line that has
                                // voice stays up until that voice finishes.
                                let lim = if cmd_skip {
                                    6
                                } else {
                                    menu::auto_hold_ms(settings.auto_speed, last_text.chars().count())
                                };
                                let voice_open = cmd_auto && !cmd_skip && voice_stream
                                    .map(|slot| mixer.as_ref().is_some_and(|mx| !mx.finished(slot)))
                                    .unwrap_or(false);
                                if !voice_open && dwell >= lim {
                                    break 'wait;
                                }
                            }
                            win.delay(16);
                        }
                    }
                vm::lscr::Event::Jump { target, kind } => {
                    eprintln!("  {} -> {}", kind, target);
                    // The clip from the scene we just skipped must not keep
                    // playing over the next one.
                    if cmd_skip {
                        skipped_voice = None;
                        if let Some(mx) = mixer.as_ref() {
                            stop_voice(mx, &mut voice_stream);
                        }
                    }
                    current_id = target;
                    // new scene: standing sprites do not carry over
                    slots = Default::default();
                    focus = None;
                    bank.as_mut().map(|b| b.cache.clear());
                    break;
                }
                vm::lscr::Event::End => {
                    eprintln!("scene {} ended", current_id);
                    return Ok(None);
                }
            }
        }
    }
}

/// Snapshot of the engine for a save slot, taken while it waits for input.
///
/// Only ids and names go in: the decoded background, the tint LUT, the sprite
/// images and the mixer slots are all re-created on load.
fn make_save(
    it: &vm::lscr::Interp,
    scene: &str,
    bg: Option<(i64, u32)>,
    tint: Option<String>,
    slots: &[Option<ActiveSprite>; SPRITE_SLOTS],
    focus: Option<u32>,
    text: &str,
    bgm: Option<i32>,
) -> save::SaveState {
    save::SaveState {
        scene: scene.to_string(),
        vm: it.snapshot(),
        bg,
        tint,
        slots: slots
            .iter()
            .map(|s| s.as_ref().map(|a| (a.char_id, a.name.clone())))
            .collect(),
        focus,
        text: text.to_string(),
        bgm,
        time: save::now(),
        flags: Vec::new(),
    }
}

/// Result of handling a command-bar button that opens a modal screen.
enum CmdOutcome {
    /// Screen closed, keep waiting for input.
    Stay,
    /// A save slot was loaded: restart the run with it.
    Loaded(save::SaveState),
    /// Player chose to quit.
    Quit,
}

/// Handle a command-bar button that opens a modal screen (保存/读档/设置/履历).
/// The toggle buttons (自动/跳过/隐藏) are handled inline by the caller.
fn handle_cmd(
    cmd: cmdbar::Cmd,
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    settings: &mut menu::Settings,
    mixer: Option<&audio::Mixer>,
    it: &vm::lscr::Interp,
    current_id: &str,
    bg_id: Option<(i64, u32)>,
    tint_name: &Option<String>,
    slots: &[Option<ActiveSprite>; SPRITE_SLOTS],
    focus: Option<u32>,
    last_text: &str,
    bgm: Option<i32>,
    frame: &render::Surface,
    backlog: &[String],
) -> CmdOutcome {
    match cmd {
        cmdbar::Cmd::QuickSave => {
            let st = make_save(
                it,
                current_id,
                bg_id,
                tint_name.clone(),
                slots,
                focus,
                last_text,
                bgm,
            );
            if let Err(e) = st.save_to(&save::quick_path()) {
                eprintln!("qsave: {}", e);
            }
            CmdOutcome::Stay
        }
        cmdbar::Cmd::QuickLoad => {
            if let Some(st) = save::SaveState::load_from(&save::quick_path()) {
                CmdOutcome::Loaded(st)
            } else {
                eprintln!("qload: no quick save");
                CmdOutcome::Stay
            }
        }
        cmdbar::Cmd::Save => {
            let st = make_save(
                it,
                current_id,
                bg_id,
                tint_name.clone(),
                slots,
                focus,
                last_text,
                bgm,
            );
            saveload::run(win, &s.bg_dirs(), saveload::Mode::Save, Some(st), Some(frame));
            CmdOutcome::Stay
        }
        cmdbar::Cmd::Load => {
            if let Some(st) =
                saveload::run(win, &s.bg_dirs(), saveload::Mode::Load, None, Some(frame))
            {
                CmdOutcome::Loaded(st)
            } else {
                CmdOutcome::Stay
            }
        }
        cmdbar::Cmd::Config => {
            let mut st = menu::Settings::load();
            menu::run(win, &s.bg_dirs(), &mut st, mixer);
            *settings = menu::Settings::load();
            menu::apply(settings, mixer);
            CmdOutcome::Stay
        }
        cmdbar::Cmd::Backlog => {
            show_backlog(win, font, backlog);
            CmdOutcome::Stay
        }
        // toggles are handled by the caller
        _ => CmdOutcome::Stay,
    }
}

/// 履历 (backlog) viewer: scroll through recently shown lines.
fn show_backlog(win: &mut sdl2::Sdl, font: &render::Font, lines: &[String]) {
    const W: u32 = 1280;
    const H: u32 = 720;
    if lines.is_empty() {
        return;
    }
    let cell = font.cell() as i32;
    let line_h = cell + 6;
    let per_page = ((H as i32 - 80) / line_h) as usize;
    let total = lines.len();
    let mut top = total.saturating_sub(per_page).max(0);
    loop {
        let mut surf = render::Surface::new(W, H, [6, 8, 16, 245]);
        surf.fill_rect(20, 16, W - 40, H - 32, [12, 16, 30, 235]);
        surf.fill_rect(20, 16, W - 40, 2, [150, 180, 230, 255]);
        let mut y = 40;
        for i in top..total.min(top + per_page) {
            font.draw(&mut surf, &lines[i], 40, y);
            y += line_h;
        }
        font.draw(&mut surf, "↑/↓ 翻页 · Enter/Esc 关闭", 40, H as i32 - 36);
        win.present(&surf);
        match win.poll_input() {
            sdl2::Input::Quit => return,
            sdl2::Input::Up => {
                if top > 0 {
                    top -= 1;
                }
            }
            sdl2::Input::Down => {
                if top + per_page < total {
                    top += 1;
                }
            }
            sdl2::Input::Advance | sdl2::Input::Back | sdl2::Input::RightClick => return,
            _ => win.delay(16),
        }
    }
}

/// Load and parse an LSCR program (`.bnr` + companion `.txt` token pool).
fn load_program(script_dir: &std::path::Path, id: &str) -> Option<vm::lscr::Program> {
    let bnr = std::fs::read(script_dir.join(format!("{}.bnr", id))).ok()?;
    let txt = std::fs::read(script_dir.join(format!("{}.txt", id))).ok()?;
    match vm::lscr::Program::load(&bnr, &txt) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("lscr parse error for {}: {}", id, e);
            None
        }
    }
}

/// Locate a directory of loose `*.tga` character sprites (legacy layout helper).
///
/// The data tree passes its own sprite directory explicitly; this is only used
/// by the explicit-path CLI form, where it tries the common layouts.
fn find_sprite_dir(bg_dir: &std::path::Path, script_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("WA2_SPRITES") {
        let pb = std::path::PathBuf::from(p);
        if pb.is_dir() {
            return Some(pb);
        }
    }
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    for base in [
        bg_dir.to_path_buf(),
        bg_dir.join(".."),
        script_dir.to_path_buf(),
        script_dir.join(".."),
    ] {
        cands.push(base.join("sprite"));
        cands.push(base.join("char"));
    }
    cands.into_iter().find(|p| p.is_dir())
}

/// Character id -> sprite file prefix.
///
/// Taken from the game exe's own prefix table (the NUL-separated list at file
/// offset 661252: `mas yan miy saw suz kiz mat ham yos ueh mhh mih aco say nak
/// hon sat tom shi you oto mam pap chi ioo tak mar izu koh set kaz har tan`),
/// cross-checked against every `(series, variant)` request in the shipped
/// scripts: for ids 1,2,3,4,5,10,11,15,16,19,23,25,28 the file set matches
/// exactly one prefix, and the rest follow the table order.
///
/// The previous hand-written table had 12 and 17 both pointing at `oto` (= 孝宏),
/// which made those two characters render as the same person, and 18 at `kaz`.
fn sprite_prefixes(cid: u32) -> &'static [&'static str] {
    match cid {
        0 => &["har"],  // 春希
        1 => &["kaz"],  // 冬馬かずさ
        2 => &["set"],  // 小木曽雪菜
        3 => &["koh"],  // 杉浦小春
        4 => &["izu"],  // 和泉千晶
        5 => &["mar"],  // 風岡麻理
        10 => &["tak"], // 飯塚武也
        11 => &["ioo"], // 水沢依緒
        12 => &["chi"], // 親志
        13 => &["pap"], // 雪菜の父
        14 => &["mam"], // 雪菜の母
        15 => &["oto"], // 孝宏
        16 => &["you"], // 冬馬曜子
        17 => &["shi"], // 担任教師
        19 => &["tom"], // 朋
        20 => &["sat"], // 佐藤
        21 => &["hon"], // 本田
        22 => &["nak"], // 中川
        23 => &["say"], // 早百合
        24 => &["aco"], // 亜子
        25 => &["mih"], // 美穂子
        26 => &["mhh"],
        27 => &["ueh"], // 上原
        28 => &["yos"],
        30 => &["ham"], // 浜田
        31 => &["mat"], // 松岡
        32 => &["kiz"], // 木崎
        33 => &["suz"], // 鈴木
        34 => &["saw"],
        35 => &["miy"],
        36 => &["yan"],
        _ => &[],
    }
}

/// Character sprite bank backed by a directory of loose `*.tga` files.
///
/// On construction it indexes the directory once, so resolution is an in-memory
/// lookup. (The previous archive-backed version sliced filenames by byte index,
/// which panicked on the archives' one non-ASCII entry name; a directory scan
/// and a prefix rule that walks `char`s cannot panic.)
struct SpriteBank {
    dir: std::path::PathBuf,
    /// lowercased file names present in the directory
    index: std::collections::HashSet<String>,
    /// Every prefix in the directory, in deterministic order.
    all_prefixes: Vec<String>,
    cache: std::collections::HashMap<String, codec::tga::TgaImage>,
}

impl SpriteBank {
    fn new(dir: std::path::PathBuf) -> SpriteBank {
        let mut index = std::collections::HashSet::new();
        let mut pset = std::collections::BTreeSet::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = match e.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let lower = name.to_ascii_lowercase();
                let stem = match lower.strip_suffix(".tga") {
                    Some(s) => s.to_string(),
                    None => continue,
                };
                // Prefix = the leading ASCII letters ("tak013101" -> "tak").
                // Char-based, so non-ASCII names cannot split a codepoint.
                let p: String = stem.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                if !p.is_empty() {
                    pset.insert(p);
                }
                index.insert(lower);
            }
        }
        SpriteBank {
            dir,
            index,
            all_prefixes: pset.into_iter().collect(),
            cache: Default::default(),
        }
    }

    fn get(&mut self, name: &str) -> Option<&codec::tga::TgaImage> {
        let key = name.to_ascii_lowercase();
        if !self.cache.contains_key(&key) {
            let data = std::fs::read(self.dir.join(&key)).ok()?;
            let img = codec::tga::decode(&data).ok()?;
            self.cache.insert(key.clone(), img);
        }
        self.cache.get(&key)
    }

    /// Resolve `(char, series, variant)` to a real sprite file name.
    ///
    /// The shipped scripts' `(series, variant)` values match the file names
    /// directly for the vast majority of cues, so the search walks *outward*
    /// from the exact value (0, -1, +1, -2, +2). The previous order started at
    /// -2, which made almost every pose collapse onto `<prefix>001101.tga` (the
    /// variant-2 file, or the 101 fallback) - standing sprites then never
    /// changed expression, and characters sharing a prefix (10/15 -> tak,
    /// 12/17 -> oto) became indistinguishable duplicates on screen.
    ///
    /// The neutral-pose fallback (`...101`) is only tried after the whole
    /// neighbourhood search has failed.
    fn resolve(&mut self, cid: u32, series: u32, variant: u32) -> Option<String> {
        let mut prefixes: Vec<String> =
            sprite_prefixes(cid).iter().map(|s| s.to_string()).collect();
        for p in &self.all_prefixes {
            if !prefixes.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                prefixes.push(p.clone());
            }
        }
        const DELTAS: [i32; 5] = [0, -1, 1, -2, 2];
        let s_base = series as i32;
        let v_base = variant as i32;
        for pf in &prefixes {
            for ds in DELTAS {
                let s = s_base + ds;
                if s < 0 {
                    continue;
                }
                for dv in DELTAS {
                    let v = v_base + dv;
                    if v < 0 {
                        continue;
                    }
                    let name = format!("{}{:03}{:03}.tga", pf, s as u32, v as u32).to_ascii_lowercase();
                    if self.index.contains(&name) {
                        return Some(name);
                    }
                }
            }
        }
        // Last resort: the character's neutral pose.
        for pf in &prefixes {
            for s in [s_base, 1i32] {
                if s < 0 {
                    continue;
                }
                for v in [101u32, 102, 100] {
                    let name = format!("{}{:03}{:03}.tga", pf, s as u32, v).to_ascii_lowercase();
                    if self.index.contains(&name) {
                        return Some(name);
                    }
                }
            }
        }
        None
    }

}

/// A character currently on screen.
struct ActiveSprite {
    char_id: u32,
    #[allow(dead_code)]
    name: String,
    img: codec::tga::TgaImage,
}

/// Horizontal anchor for the idx-th of `count` on-screen characters.
fn sprite_x_for(idx: usize, count: usize, w: u32) -> i32 {
    let center = match (count, idx) {
        (1, _) => 640,
        (2, 0) => 360,
        (2, 1) => 920,
        (3, 0) => 330,
        (3, 1) => 640,
        (3, 2) => 950,
        (_, i) => 330 + (i.min(2) as i32) * 310,
    };
    center - (w as i32 / 2)
}

/// Composite background + sprites + dialog box + text.
fn draw_scene(
    bg: &codec::tga::TgaImage,
    text: &str,
    speaker: Option<&str>,
    font: &render::Font,
    slots: &[Option<ActiveSprite>; SPRITE_SLOTS],
    focus: Option<u32>,
    w: u32,
    h: u32,
    tint: Option<&[u8]>,
    // 「ウィンドウ一時消去」: false hides the dialog box and its text.
    window: bool,
    plate: Option<&codec::tga::TgaImage>,
    frame: Option<&codec::tga::TgaImage>,
) -> render::Surface {
    let multi = slots.iter().filter(|s| s.is_some()).count() > 1;
    let draws: Vec<render::SpriteDraw> = SPRITE_DRAW_ORDER
        .iter()
        .filter_map(|&i| {
            let s = slots[i].as_ref()?;
            // Dim the characters that are not speaking.
            let alpha = if multi && focus.is_some() && focus != Some(s.char_id) {
                170
            } else {
                255
            };
            Some(render::SpriteDraw {
                img: &s.img,
                x: SPRITE_ANCHORS[i] - s.img.width as i32 / 2,
                y: (h as i32 - s.img.height as i32).max(0),
                alpha,
            })
        })
        .collect();
    let mut s = render::render_scene_ex(bg, text, font, w, h, &draws, tint, window, plate, frame);
    // The name sits on the upper rim of the oval, not inside 【】 in the line.
    if window {
        if let Some(name) = speaker {
            if !name.is_empty() && plate.is_some() {
                font.draw_pitch(&mut s, name, render::NAME_X, render::NAME_Y, render::DIALOG_PITCH);
            }
        }
    }
    s
}

/// Older saves stored the line as `【name】\nbody`. Split that so a loaded
/// slot still puts the name on the rim.
fn split_saved_line(text: &str) -> (Option<String>, String) {
    if let Some(rest) = text.strip_prefix('【') {
        if let Some((name, body)) = rest.split_once("】\n") {
            return (Some(name.to_string()), body.to_string());
        }
    }
    (None, text.to_string())
}

/// Clean scenario text for rendering: literal `\n` -> newline, `\k` removed,
/// ruby tags `<Rbase|reading>` collapsed to `base`, other `<...>` tags stripped.
fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('k') => {
                    chars.next();
                }
                _ => out.push(c),
            },
            '<' => {
                let mut tag = String::new();
                loop {
                    match chars.next() {
                        Some('>') | None => break,
                        Some(d) => tag.push(d),
                    }
                }
                if let Some(rest) = tag.strip_prefix('R') {
                    if let Some(base) = rest.split('|').next() {
                        out.push_str(base);
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// What the title screen resolved to.
enum TitleAction {
    Start,
    /// Resume from a save slot picked on the load screen.
    Load(save::SaveState),
    Quit,
}

/// Original title screen, composed from the game's own title graphics.
///
/// Because the menu is drawn from image atlases instead of engine-side strings,
/// it is automatically in the game's language - the Chinese patch ships
/// translated copies of every one of these images and they win over the
/// Japanese ones in `bg_dirs`.
///
///   T0011  752x448   logo (transparent; the backdrop is its edge colour #011D59)
///   T0100  640x640   menu atlas: 16 rows x 2 columns of 320x40 tiles,
///                    left column = normal (white), right = selected (gold)
///   T0101  320x80    highlight bar: two 320x40 frames (dim / bright)
///   T0020  264x24    copyright line
///
/// The bar slides between items and flashes its bright frame while it moves,
/// which is the selection feedback the original has. Arrow keys move,
/// Space/Enter confirms, Esc quits; items whose subsystems do not exist yet
/// answer with a notice.
///
/// `WA2_TITLE_AUTO=<n>` pre-selects item `n` and confirms it (verification runs).
fn run_title(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    bg_dirs: &[&std::path::Path],
    cn: bool,
) -> TitleAction {
    const W: u32 = 1280;
    const H: u32 = 720;
    const ITEM_W: i32 = 320;
    const ITEM_H: i32 = 40;
    const PITCH: i32 = 40;
    const N: usize = 5;
    const X0: i32 = (W as i32 - ITEM_W) / 2;
    const Y0: i32 = 432;
    const BACKDROP: [u8; 4] = [1, 29, 89, 255]; // T0011's edge colour

    // Only used when the menu atlas is missing from the data tree.
    let fallback_items = if cn {
        ["开始游戏", "继续游戏", "系统设置", "特别模式", "结束游戏"]
    } else {
        ["はじめから", "つづきから", "オプション", "スペシャル", "ゲーム終了"]
    };
    let notice_text = if cn { "此功能尚未实现" } else { "ただいま準備中です" };

    let logo = load_background(bg_dirs, "T0011.tga");
    let copyright = load_background(bg_dirs, "T0020.tga");
    let menu = load_background(bg_dirs, "T0100.tga");
    let bar = load_background(bg_dirs, "T0101.tga");
    if menu.is_none() {
        eprintln!("title: T0100.tga not found, falling back to engine-drawn menu text");
    }

    if std::env::var_os("WA2_DEBUG_INPUT").is_some() {
        let (ww, wh) = win.window_size();
        eprintln!("title: window_size = {}x{}", ww, wh);
    }
    // 「画面模式」 applies as soon as the option screen is left.
    let startup = menu::Settings::load();
    win.set_fullscreen(startup.win_disp == 1);

    let item_y = |i: usize| (Y0 + i as i32 * PITCH) as f32;
    let mut sel = 0usize;
    let mut notice: Option<&str> = None;
    let mut bar_y = item_y(0);
    let mut flash_until = std::time::Instant::now();
    let mut auto_pick: Option<usize> = std::env::var_os("WA2_TITLE_AUTO")
        .and_then(|v| v.to_string_lossy().parse::<usize>().ok())
        .map(|p| p.min(N - 1));
    if let Some(p) = auto_pick {
        sel = p;
        bar_y = item_y(p);
    }

    loop {
        // ---- input (non-blocking so the bar can animate) --------------------
        let mut confirm = false;
        if auto_pick.is_some() {
            auto_pick = None;
            win.delay(600);
            confirm = true;
        } else {
            match win.poll_input() {
                sdl2::Input::Up => {
                    sel = (sel + N - 1) % N;
                    flash_until = std::time::Instant::now() + std::time::Duration::from_millis(160);
                }
                sdl2::Input::Down => {
                    sel = (sel + 1) % N;
                    flash_until = std::time::Instant::now() + std::time::Duration::from_millis(160);
                }
                sdl2::Input::Advance => confirm = true,
                sdl2::Input::Choice(k) => {
                    sel = ((k as usize).saturating_sub(1)).min(N - 1);
                    flash_until = std::time::Instant::now() + std::time::Duration::from_millis(160);
                    confirm = true;
                }
                sdl2::Input::Quit => return TitleAction::Quit,
                // Pointer: hovering highlights the item under the cursor, a
                // click on an item confirms it. The original is mouse-driven,
                // so this is the primary way to pick a menu entry.
                sdl2::Input::Pointer { x, y, pressed } => {
                    let (sx, sy) = win.surface_pos(x, y, W, H);
                    if let Some(i) = (0..N).find(|i| {
                        let iy = Y0 + *i as i32 * PITCH;
                        sx >= X0 && sx < X0 + ITEM_W && sy >= iy && sy < iy + ITEM_H
                    }) {
                        if i != sel {
                            sel = i;
                            flash_until =
                                std::time::Instant::now() + std::time::Duration::from_millis(160);
                        }
                        if pressed {
                            confirm = true;
                        }
                    }
                }
                _ => {}
            }
        }

        // ---- animate the highlight bar -------------------------------------
        let target = item_y(sel);
        if (bar_y - target).abs() > 0.5 {
            bar_y += (target - bar_y) * 0.30;
        } else {
            bar_y = target;
        }
        let flashing = std::time::Instant::now() < flash_until;

        // ---- compose --------------------------------------------------------
        let mut s = render::Surface::new(W, H, BACKDROP);
        if let Some(ref lg) = logo {
            s.blit(lg, (W as i32 - lg.width as i32) / 2, 24);
        }
        // Highlight bar first: the item tiles are transparent except for the
        // glyphs, so it shows through behind the selected label.
        if let Some(ref b) = bar {
            let frame = if flashing { ITEM_H } else { 0 };
            s.blit_region(b, 0, frame, ITEM_W as u32, ITEM_H as u32, X0, bar_y as i32);
        }
        match menu {
            Some(ref m) => {
                for i in 0..N {
                    let col = if i == sel { ITEM_W } else { 0 };
                    s.blit_region(
                        m,
                        col,
                        i as i32 * ITEM_H,
                        ITEM_W as u32,
                        ITEM_H as u32,
                        X0,
                        Y0 + i as i32 * PITCH,
                    );
                }
            }
            None => {
                let cell = font.cell() as i32;
                for (i, item) in fallback_items.iter().enumerate() {
                    let y = Y0 + i as i32 * PITCH;
                    font.draw(&mut s, item, X0 + 40, y);
                }
            }
        }
        if let Some(n) = notice {
            // Engine-side strings are *not* renderable with the game atlas in
            // Chinese mode: the patch's text is Shift-JIS-coded pseudo-Japanese
            // (its glyphs live in repainted cells), so real Simplified Chinese
            // has no glyphs. Use the system CJK font for this one label and fall
            // back to the bitmap font if it is unavailable.
            let mut drawn = false;
            if cn {
                let strip_h = font.cell() + 24;
                if let Some((rw, rh, rgba)) =
                    render::text::block_rgba(&[n.to_string()], 640, strip_h, 26, render::text::DEFAULT_FONT)
                {
                    let nx = W as i32 / 2 - rw as i32 / 2;
                    s.fill_rect(nx - 16, 366, rw + 32, rh + 8, [1, 29, 89, 235]);
                    s.blit_rgba(&rgba, rw, rh, nx, 370);
                    drawn = true;
                }
            }
            if !drawn {
                let cell = font.cell() as i32;
                let wpx = n.chars().count() as i32 * cell;
                let nx = W as i32 / 2 - wpx / 2;
                s.fill_rect(nx - 16, 372, (wpx + 32) as u32, (cell + 12) as u32, [1, 29, 89, 235]);
                font.draw(&mut s, n, nx, 378);
            }
        }
        if let Some(ref cp) = copyright {
            s.blit(cp, W as i32 - cp.width as i32 - 24, H as i32 - cp.height as i32 - 14);
        }
        win.present(&s);

        // ---- act on the confirmation ---------------------------------------
        if confirm {
            if notice.is_some() {
                notice = None;
                continue;
            }
            match sel {
                0 => return TitleAction::Start,
                // 「继续游戏」: the load screen. A picked slot resumes the game.
                1 => {
                    if let Some(snap) = saveload::run(win, bg_dirs, saveload::Mode::Load, None, None) {
                        return TitleAction::Load(snap);
                    }
                    notice = None;
                }
                2 => {
                    // 「系统设置」: the game's own option screen.
                    let mut st = menu::Settings::load();
                    menu::apply(&st, None);
                    menu::run(win, bg_dirs, &mut st, None);
                    win.set_fullscreen(st.win_disp == 1);
                    notice = None;
                }
                4 => return TitleAction::Quit,
                _ => notice = Some(notice_text),
            }
        }
        win.delay(16);
    }
}

/// Play a movie (`mvNN` token) from the movie directory.
///
/// The movies are raw ASF/WMV files (the original `mv*.pak` is not an archive:
/// its first bytes are the ASF GUID), so the data tree stores them as plain
/// `*.wmv`. Scripts name them "mv01".."mv24" while the files ship as version
/// pairs (`mv010`, `mv011`), so we try the plain name first, then 0 and 1.
fn play_movie(win: &mut sdl2::Sdl, movie_dir: &std::path::Path, name: &str) -> Option<()> {
    let cands = [
        name.to_ascii_lowercase(),
        format!("{}0", name.to_ascii_lowercase()),
        format!("{}1", name.to_ascii_lowercase()),
    ];
    let mut path = None;
    'outer: for cand in &cands {
        for ext in [".wmv", ".asf", ".WMV", ".ASF", ".pak", ".PAK"] {
            let p = movie_dir.join(format!("{}{}", cand, ext));
            if p.is_file() {
                path = Some(p);
                break 'outer;
            }
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("  movie {} not found in {} (skipped)", name, movie_dir.display());
            return None;
        }
    };
    let mut movie = video::Movie::open(path.to_str()?)?;
    eprintln!("  playing movie {} ({}x{} @ {:.2}fps)", name, movie.width, movie.height, movie.fps);
    let mut frame = vec![0u8; (movie.width * movie.height * 4) as usize];
    let delay = std::time::Duration::from_secs_f64(1.0 / movie.fps.max(1.0));
    while movie.read_frame(&mut frame) {
        win.present_raw(&frame, movie.width, movie.height);
        std::thread::sleep(delay);
        match win.poll_input() {
            sdl2::Input::None | sdl2::Input::Pointer { pressed: false, .. } => {}
            _ => break,
        }
    }
    Some(())
}

/// Block until a non-None input event arrives.
fn wait_input(win: &mut sdl2::Sdl) -> sdl2::Input {
    loop {
        let ev = win.poll_input();
        match ev {
            sdl2::Input::None | sdl2::Input::Pointer { pressed: false, .. } => {}
            other => return other,
        }
        win.delay(16);
    }
}

/// Load the font for a data tree: atlas + authoritative cell map.
///
/// The cell map (`wa2_cell_map_{jp,cn}.tsv`) is the game exe's own font table
/// expressed as atlas cells, so glyph lookup is exact for both the Japanese and
/// the Chinese atlas - no heuristics, no codec ambiguity.
fn load_font(d: &data::Data) -> Option<render::Font> {
    load_font_paths(&d.atlas(), Some(&d.cell_map()), None)
}

/// Load a font atlas with either an authoritative cell map, a legacy charset
/// table, or both (the cell map wins).
fn load_font_paths(
    atlas_path: &std::path::Path,
    cell_map: Option<&std::path::Path>,
    charset_path: Option<&std::path::Path>,
) -> Option<render::Font> {
    let atlas = match std::fs::read(atlas_path).ok().and_then(|d| codec::tga::decode_font(&d).ok()) {
        Some(i) => i,
        None => { eprintln!("error: could not decode font atlas {}", atlas_path.display()); return None; }
    };
    let cell = if atlas.width >= 3000 { 40 } else { 24 };
    eprintln!("font atlas {}x{} -> cell={}", atlas.width, atlas.height, cell);

    if let Some(map) = cell_map {
        match render::Font::from_cell_map(atlas, cell, map) {
            Ok(f) => {
                eprintln!("font cell map: {} ({} glyphs)", map.display(), f.mapped_glyphs());
                return Some(f);
            }
            Err(e) => {
                eprintln!("error reading cell map {}: {}", map.display(), e);
                return None;
            }
        }
    }

    let charset_path = charset_path?;
    let charset = match std::fs::read_to_string(charset_path) {
        Ok(s) => s,
        Err(e) => { eprintln!("error reading {}: {}", charset_path.display(), e); return None; }
    };
    // Two charset flavours need different handling:
    //  * a 1:1 atlas table (SJIS-ordered charset.txt, ~7.7k chars) whose
    //    leading entries are REAL blank/space glyphs. Stripping whitespace
    //    there shifts every glyph by one cell (cell 0 = space, cell 1 = !).
    //  * a formatted charset (wa2.cht.txt / wa2_cset.txt) containing wrapping
    //    newlines/spaces, which must be stripped; those start at cell 748.
    let ws = charset.chars().filter(|c| c.is_whitespace()).count();
    if ws > 10 {
        eprintln!("font charset: formatted ({} ws stripped) start_pos=748", ws);
        Some(render::Font::from_charset(atlas, &charset, cell, 748))
    } else {
        eprintln!("font charset: 1:1 table ({} ws kept) start_pos=0", ws);
        Some(render::Font::from_charset_raw(atlas, &charset, cell, 0))
    }
}

/// Legacy 2-argument form used by the single-scene debugging commands.
fn load_font_charset(atlas_path: &str, charset_path: &str) -> Option<render::Font> {
    load_font_paths(
        std::path::Path::new(atlas_path),
        None,
        Some(std::path::Path::new(charset_path)),
    )
}

/// Resolve the output directory: explicit arg if present, else `<stem><suffix>`.
fn out_dir(pak: &str, args: &[String], explicit_idx: usize, suffix: &str) -> String {
    if args.len() > explicit_idx { return args[explicit_idx].clone(); }
    let p = Path::new(pak);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "out".into());
    format!("{}{}", stem, suffix)
}

fn print_usage() {
    println!("wa2-native - native engine for White Album 2 (AVG32)");
    println!("usage:");
    println!("  wa2-native kcap <pak> [out]     unpack a KCAP data archive");
    println!("  wa2-native lac  <pak> [out]     unpack a LAC audio archive");
    println!("  wa2-native extract <pak> [out]  auto-detect and unpack KCAP/LAC");
    println!("  wa2-native tga <file.tga>       verify a TGA decodes");
    println!("  wa2-native sjis <file>          decode CP932 bytes to UTF-8");
    println!("  wa2-native text-info <txt>      parse a scenario text page");
    println!("  wa2-native scene <txt> <font> <charset> <bg.tga> <out.tga>  render a scene");
}
