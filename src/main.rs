mod archive;
mod audio;
mod codec;
mod data;
mod render;
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
            let surf = render::render_scene(&bg, &page.text(), &font, 1280, 720);
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
                        vm::lscr::Event::Text { text, speaker, voice_tok } => {
                            let body = clean_text(&text).replace('\n', " / ");
                            let sp = speaker.unwrap_or_default();
                            let vt = voice_tok.map(|v| format!("v{}", v)).unwrap_or_default();
                            println!("  [text] speaker={:?} voice={} | {}", sp, vt, body);
                        }
                        vm::lscr::Event::Bg { name } => println!("  [bg  ] {}", if name.is_empty() { "<black>" } else { &name }),
                        vm::lscr::Event::Movie { name } => println!("  [mv  ] {}", name),
                        vm::lscr::Event::Sprite { char_id, series, variant } =>
                            println!("  [spr ] char={} series={} variant={}", char_id, series, variant),
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

/// Voice file lookup, indexed once instead of scanning the directory per line.
///
/// Voice files are named `{scenario}_{line:04d}_{XX}.OGG`, where `line` is the
/// token index of the speaker token; `XX` is a per-character variant, so the
/// first file for a `{scenario}_{line}` key wins.
struct VoiceIndex {
    map: std::collections::HashMap<String, std::path::PathBuf>,
}

impl VoiceIndex {
    fn build(dir: &std::path::Path) -> VoiceIndex {
        let mut map = std::collections::HashMap::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = match e.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                // "1008_0199_04.OGG" -> "1008_0199"
                let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
                let mut parts = stem.splitn(3, '_');
                let key = match (parts.next(), parts.next()) {
                    (Some(a), Some(b)) => format!("{}_{}", a, b),
                    _ => continue,
                };
                map.entry(key).or_insert_with(|| e.path());
            }
        }
        VoiceIndex { map }
    }

    fn find(&self, scenario: &str, line: u32) -> Option<&std::path::Path> {
        self.map.get(&format!("{}_{:04}", scenario, line)).map(|p| p.as_path())
    }
}

/// Build a solid-color fallback background image.
fn solid_bg(w: u32, h: u32, color: [u8; 3]) -> codec::tga::TgaImage {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        rgba.extend_from_slice(&[color[0], color[1], color[2], 255]);
    }
    codec::tga::TgaImage { width: w, height: h, rgba }
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
fn run_game(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    start_id: &str,
) -> Result<(), String> {
    let script_dir = s.script_dir.as_path();
    let bg_dir = s.bg_dir.as_path();
    let voice_dir = s.voice_dir.as_deref();
    let bgm_path = s.bgm.as_deref();
    const W: u32 = 1280;
    const H: u32 = 720;
    let mut current_id = start_id.to_string();
    // Black until the script sets one: WA2 opens on a black screen (the IC
    // prologue's "sepia.AMP" is a color-filter LUT, not a background image).
    let mut bg = solid_bg(W, H, [0, 0, 0]);

    // Start BGM if requested (and keep mixer for voice playback).
    let mut mixer = if bgm_path.is_some() || voice_dir.is_some() {
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

    let mut cur_surf = render::render_scene(&bg, "", font, W, H);
    let mut bank = s.sprite_dir.as_ref().filter(|p| p.is_dir()).map(|dir| {
        eprintln!("sprites: {}", dir.display());
        SpriteBank::new(dir.clone())
    });
    if bank.is_none() {
        eprintln!("note: no sprite directory - character sprites disabled");
    }
    let mut sprites: Vec<ActiveSprite> = Vec::new();
    let mut focus: Option<u32> = None;
    loop {
        let prog = match load_program(script_dir, &current_id) {
            Some(p) => p,
            None => {
                eprintln!("scene {} not found (.bnr/.txt)", current_id);
                return Ok(());
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
        loop {
            match it.next_event() {
                vm::lscr::Event::Text { text, speaker, voice_tok } => {
                    let body = clean_text(&text);
                    let display = match speaker {
                        Some(sp) => format!("【{}】\n{}", sp, body),
                        None => body,
                    };
                    // Voice playback keyed by the speaker token index.
                    if let (Some(vi), Some(vt), Some(mx)) = (voices.as_ref(), voice_tok, mixer.as_ref()) {
                        if let Some(vf) = vi.find(&current_id, vt) {
                            mx.play(&vf.to_string_lossy(), 1.0, false);
                        }
                    }
                    cur_surf = draw_scene(&bg, &display, font, &sprites, focus, W, H);
                    win.present(&cur_surf);
                }
                vm::lscr::Event::Sprite { char_id, series, variant } => {
                    if let Some(bank) = bank.as_mut() {
                        if let Some(name) = bank.resolve(char_id, series, variant) {
                            if let Some(img) = bank.get(&name) {
                                let img = img.clone();
                                eprintln!("  sprite: {} (char {})", name, char_id);
                                if let Some(sp) = sprites.iter_mut().find(|s| s.char_id == char_id) {
                                    sp.img = img;
                                    sp.name = name;
                                } else {
                                    if sprites.len() >= 3 {
                                        sprites.remove(0);
                                    }
                                    sprites.push(ActiveSprite { char_id, name, img });
                                }
                            }
                        } else {
                            eprintln!("  sprite: char {} series {:03} variant {:03} not found", char_id, series, variant);
                        }
                    }
                }
                vm::lscr::Event::SpriteFocus { char_id } => {
                    focus = Some(char_id);
                }
                vm::lscr::Event::Bg { name } => {
                    // `.AMP` tokens are palette/effect tints we do not apply; the
                    // empty token clears to black.
                    if name.is_empty() {
                        eprintln!("  bg: <black>");
                        bg = solid_bg(W, H, [0, 0, 0]);
                    } else if name.to_ascii_lowercase().ends_with(".tga") {
                        match load_background(&s.bg_dirs(), &name) {
                            Some(img) => {
                                eprintln!("  bg: {} ({}x{})", name, img.width, img.height);
                                bg = img;
                            }
                            // Missing files are the norm for the overlay layers of
                            // a layered CG (`v100200_bg.tga` etc. ship only inside
                            // the base image), so say so once instead of silently
                            // keeping the previous background.
                            None => eprintln!("  bg: {} NOT FOUND", name),
                        }
                    } else {
                        eprintln!("  bg: {} (effect, ignored)", name);
                    }
                    cur_surf = draw_scene(&bg, "", font, &sprites, focus, W, H);
                    // Show it immediately: a background change is a visible event
                    // on its own (the original engine repaints here), and the next
                    // script op may be a movie that takes over the screen.
                    win.present(&cur_surf);
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
                    let cell = font.cell() as i32;
                    let line_h = cell + 4;
                    let box_h = (H / 4).max(140);
                    let box_y = (H - box_h - 24) as i32;
                    let opt_top = |i: usize| box_y + 18 + (i as i32 + 1) * line_h;
                    let base = draw_scene(&bg, &body, font, &sprites, focus, W, H);
                    let mut hover: Option<usize> = None;
                    cur_surf = draw_scene(&bg, &body, font, &sprites, focus, W, H);
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
                                frame.fill_rect(40, y, W - 80, (line_h + 4) as u32, [130, 190, 255, 70]);
                            }
                            win.present(&frame);
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(()),
                                sdl2::Input::Choice(n) if (n as usize) <= opts.len() => {
                                    break n as u32
                                }
                                sdl2::Input::Pointer { x, y, pressed } => {
                                    let (_, sy) = win.surface_pos(x, y, W, H);
                                    let hit = (0..opts.len()).find(|i| {
                                        sy >= opt_top(*i) - 6 && sy < opt_top(*i) + line_h
                                    });
                                    hover = hit;
                                    if pressed {
                                        if let Some(i) = hit {
                                            break (i + 1) as u32;
                                        }
                                    }
                                    win.delay(16);
                                }
                                _ => win.delay(16),
                            }
                        }
                    };
                    eprintln!("  choice: {} / {}", chosen, opts.len());
                    it.set_choice_result(chosen);
                }

                vm::lscr::Event::Wait => {
                    win.present(&cur_surf);
                    if auto_text {
                        // auto-advance mode (demo / verification): keep stepping
                        win.delay(40);
                    } else {
                        match wait_input(win) {
                            sdl2::Input::Quit => return Ok(()),
                            _ => {}
                        }
                    }
                }
                vm::lscr::Event::Jump { target, kind } => {
                    eprintln!("  {} -> {}", kind, target);
                    current_id = target;
                    // new scene: standing sprites do not carry over
                    sprites.clear();
                    focus = None;
                    bank.as_mut().map(|b| b.cache.clear());
                    break;
                }
                vm::lscr::Event::End => {
                    eprintln!("scene {} ended", current_id);
                    return Ok(());
                }
            }
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

/// Character id -> candidate sprite name prefixes (romaji-first; refined from
/// the shipped scripts). The first prefix that yields an existing char.pak
/// entry wins.
fn sprite_prefixes(cid: u32) -> &'static [&'static str] {
    match cid {
        0 => &["har"],
        1 => &["kaz"],
        2 => &["set"],
        3 => &["koh"],
        4 => &["izu"],
        5 => &["mar"],
        10 => &["tak", "oto"],
        11 => &["ioo", "tak"],
        12 => &["oto"],
        13 => &["pap"],
        14 => &["oto", "mam"],
        15 => &["tak", "oto"],
        16 => &["you"],
        17 => &["oto"],
        18 => &["kaz"],
        19 => &["tom"],
        20 => &["Sat", "sat"],
        21 => &["Sat"],
        22 => &["nak"],
        23 => &["say"],
        24 => &["aco", "koh"],
        25 => &["mih"],
        27 => &["ueh", "mat"],
        28 => &["kiz", "yos"],
        29 => &["koh"],
        30 => &["ham", "mat"],
        31 => &["mat"],
        32 => &["koh"],
        33 => &["suz", "izu"],
        34 => &["saw"],
        35 => &["miy", "tom"],
        36 => &["koh"],
        37 => &["koh"],
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
    font: &render::Font,
    sprites: &[ActiveSprite],
    focus: Option<u32>,
    w: u32,
    h: u32,
) -> render::Surface {
    let count = sprites.len();
    let draws: Vec<render::SpriteDraw> = sprites
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let alpha = if count > 1 && focus.is_some() && focus != Some(s.char_id) {
                170
            } else {
                255
            };
            render::SpriteDraw {
                img: &s.img,
                x: sprite_x_for(i, count, s.img.width),
                y: (h as i32 - s.img.height as i32).max(0),
                alpha,
            }
        })
        .collect();
    render::render_scene_ex(bg, text, font, w, h, &draws)
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
            sdl2::Input::None => {}
            _ => break,
        }
    }
    Some(())
}

/// Block until a non-None input event arrives.
fn wait_input(win: &mut sdl2::Sdl) -> sdl2::Input {
    loop {
        let ev = win.poll_input();
        if ev != sdl2::Input::None {
            return ev;
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
    let atlas = match std::fs::read(atlas_path).ok().and_then(|d| codec::tga::decode(&d).ok()) {
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