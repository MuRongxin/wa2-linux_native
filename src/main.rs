mod archive;
mod audio;
mod codec;
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
            let font = match load_font(&args[2], &args[3]) { Some(f) => f, None => return };
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
            let font = match load_font(&args[3], &args[4]) { Some(f) => f, None => return };
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
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::None => win.delay(16),
                        sdl2::Input::Choice(_) => win.delay(16),
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
            let font = match load_font(&args[2], &args[3]) { Some(f) => f, None => return };
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
            let font = match load_font(&args[3], &args[4]) { Some(f) => f, None => return };
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
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::Choice(_) => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::None => { win.delay(16); }
                    }
                }
            }
        }
        "run" => {
            // wa2-native run <script_dir> <font.tga> <charset.txt> <bg_dir> <start_id> [bgm.ogg] [voice_dir]
            if args.len() < 7 {
                eprintln!("usage: wa2-native run <script_dir> <font.tga> <charset.txt> <bg_dir> <start_id> [bgm.ogg] [voice_dir]");
                return;
            }
            let script_dir = std::path::Path::new(&args[2]);
            let bg_dir = std::path::Path::new(&args[5]);
            let start_id = args[6].clone();
            let font = match load_font(&args[3], &args[4]) { Some(f) => f, None => return };
            let mut win = match sdl2::Sdl::new("wa2-native", 1280, 720) {
                Some(w) => w,
                None => { eprintln!("error: SDL2 unavailable / no display. Try DISPLAY=:0"); return; }
            };
            let bgm_path = args.get(7).cloned();
            let voice_dir = args.get(8).map(|s| std::path::PathBuf::from(s));
            eprintln!("wa2-native run: start={} (Space/Click=next, Left=back, 1-9=choice, Esc/q=quit)", start_id);
            if let Err(e) = run_game(&mut win, &font, script_dir, bg_dir, &start_id, bgm_path.as_deref(), voice_dir.as_deref()) {
                eprintln!("run error: {}", e);
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

/// Try to load a background TGA by name from `bg_dir`. Returns None if the
/// file is missing or fails to decode (caller keeps the previous background).
///
/// WA2 scripts sometimes reference a suffixed variant (e.g. `v100200_bg.tga`)
/// while the archive contains the base name (`v100200.tga`). We try the exact
/// name first, then strip common suffixes.
fn load_background(bg_dir: &std::path::Path, name: &str) -> Option<codec::tga::TgaImage> {
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

    for cand in candidates {
        let p = bg_dir.join(&cand);
        if let Ok(data) = std::fs::read(&p) {
            if let Ok(img) = codec::tga::decode(&data) {
                return Some(img);
            }
        }
    }
    None
}

/// Find a voice file for the given scenario and voice line index.
///
/// Voice files are named `{scenario}_{line:04d}_{XX}.OGG` where `line` is the
/// .txt field index of the speaker token. There may be multiple character
/// variants; we return the first match.
fn find_voice(voice_dir: &std::path::Path, scenario: &str, line: u32) -> Option<String> {
    let prefix = format!("{}_{:04}_", scenario, line);
    let entries = std::fs::read_dir(voice_dir).ok()?;
    for entry in entries.flatten() {
        if let Ok(name) = entry.file_name().into_string() {
            if name.starts_with(&prefix) {
                return Some(entry.path().to_string_lossy().into_owned());
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

/// Full game loop: play scenes starting from `start_id`, following CATCH jumps
/// and presenting choices to the player.
fn run_game(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    script_dir: &std::path::Path,
    bg_dir: &std::path::Path,
    start_id: &str,
    bgm_path: Option<&str>,
    voice_dir: Option<&std::path::Path>,
) -> Result<(), String> {
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

    let mut cur_surf = render::render_scene(&bg, "", font, W, H);
    let mut bank = find_char_pak(bg_dir, script_dir).map(|pak| {
        eprintln!("char.pak: {}", pak.display());
        SpriteBank { pak, cache: Default::default() }
    });
    if bank.is_none() {
        eprintln!("note: char.pak not found - character sprites disabled");
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
                    if let (Some(vd), Some(vt), Some(mx)) = (voice_dir, voice_tok, mixer.as_ref()) {
                        if let Some(vf) = find_voice(vd, &current_id, vt) {
                            mx.play(&vf, 1.0, false);
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
                        bg = solid_bg(W, H, [0, 0, 0]);
                    } else if name.to_ascii_lowercase().ends_with(".tga") {
                        if let Some(img) = load_background(bg_dir, &name) {
                            bg = img;
                        }
                    }
                    cur_surf = draw_scene(&bg, "", font, &sprites, focus, W, H);
                }
                vm::lscr::Event::Movie { name } => {
                    eprintln!("  movie: {}", name);
                    let _ = play_movie(win, bg_dir, &name);
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
                    let mut body = String::from("—— 请选择 ——\n");
                    for (i, o) in opts.iter().enumerate() {
                        body.push_str(&format!("{}. {}\n", i + 1, o));
                    }
                    cur_surf = draw_scene(&bg, &body, font, &sprites, focus, W, H);
                    win.present(&cur_surf);
                    let chosen = if std::env::var_os("WA2_AUTO").is_some() {
                        win.delay(300);
                        1u32
                    } else {
                        loop {
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(()),
                                sdl2::Input::Choice(n) if (n as usize) <= opts.len() => {
                                    break n as u32
                                }
                                sdl2::Input::None => win.delay(16),
                                sdl2::Input::Choice(_) => win.delay(16),
                                _ => {}
                            }
                        }
                    };
                    eprintln!("  choice: {} / {}", chosen, opts.len());
                    it.set_choice_result(chosen);
                }
                vm::lscr::Event::Wait => {
                    win.present(&cur_surf);
                    if std::env::var_os("WA2_AUTO").is_some() {
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

/// Locate `char.pak` (standing-character sprites) near the media directories.
fn find_char_pak(bg_dir: &std::path::Path, script_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("WA2_CHARPAK") {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
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
        cands.push(base.join("char.pak"));
        cands.push(base.join("char.PAK"));
        cands.push(base.join("IC").join("char.pak"));
    }
    cands.into_iter().find(|p| p.exists())
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

/// Lazily-loaded character sprite cache backed by `char.pak`.
struct SpriteBank {
    pak: std::path::PathBuf,
    cache: std::collections::HashMap<String, codec::tga::TgaImage>,
}

impl SpriteBank {
    fn get(&mut self, name: &str) -> Option<&codec::tga::TgaImage> {
        if !self.cache.contains_key(name) {
            let data = archive::kcap::read_entry(&self.pak, name)?;
            let img = codec::tga::decode(&data).ok()?;
            self.cache.insert(name.to_string(), img);
        }
        self.cache.get(name)
    }

    /// Resolve `(char, series, variant)` to a sprite file name, trying a few
    /// variant fallbacks when the exact file is missing from the archive.
    fn resolve(&mut self, cid: u32, series: u32, variant: u32) -> Option<String> {
        let variants = [variant, variant.saturating_sub(1), variant + 1, 101, 102];
        for pf in sprite_prefixes(cid) {
            for v in variants {
                let name = format!("{}{:03}{:03}.tga", pf, series, v);
                if self.get(&name).is_some() {
                    return Some(name);
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

/// Play a movie (`mvNN` token) found next to the background directory. Returns
/// after EOF or any key press.
fn play_movie(win: &mut sdl2::Sdl, media_dir: &std::path::Path, name: &str) -> Option<()> {
    // Scripts name movies "mv01".."mv24"; archives ship version pairs like
    // mv010.pak/mv011.pak — try the plain name first, then the 0 and 1 versions.
    let bases = [
        media_dir.to_path_buf(),
        media_dir.join(".."),
        media_dir.join("IC"),
        media_dir.join("../mv"),
    ];
    let cands = [name.to_string(), format!("{}0", name), format!("{}1", name)];
    let mut path = None;
    'outer: for base in &bases {
        for cand in &cands {
            for ext in [".pak", ".PAK"] {
                let p = base.join(format!("{}{}", cand, ext));
                if p.exists() {
                    path = Some(p);
                    break 'outer;
                }
            }
        }
    }
    let path = path?;
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

/// Load a font atlas + charset and build a Font.
///
/// Two atlas sizes exist: the 14pt sheets are 1920px wide (24px cells) and the
/// body80 sheets are 3200px wide (40px cells).
///
/// The charset file must list glyphs in atlas cell order starting at cell 0
/// (`start_pos = 0`), which is how the SJIS-ordered charset
/// (`wa2_sjis_charset.txt`, generated from the exe's font table) is laid out:
/// cell 1 = '!', cell 33 = 'A', CJK from cell ~524 on. Verified against the
/// atlas bitmaps (see the glyph checks in the port history).
fn load_font(atlas_path: &str, charset_path: &str) -> Option<render::Font> {
    let atlas = match std::fs::read(atlas_path).ok().and_then(|d| codec::tga::decode(&d).ok()) {
        Some(i) => i,
        None => { eprintln!("error: could not decode font atlas {}", atlas_path); return None; }
    };
    let cell = if atlas.width >= 3000 { 40 } else { 24 };
    eprintln!("font atlas {}x{} -> cell={}", atlas.width, atlas.height, cell);
    let charset = match std::fs::read_to_string(charset_path) {
        Ok(s) => s,
        Err(e) => { eprintln!("error reading {}: {}", charset_path, e); return None; }
    };
    Some(render::Font::from_charset(atlas, &charset, cell, 0))
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