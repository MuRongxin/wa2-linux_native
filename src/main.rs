mod archive;
mod artcache;
mod audio;
mod codec;
mod cmdbar;
mod data;
mod ft2;
mod menu;
mod render;
mod save;
mod saveload;
mod sdl2;
mod timeline;
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
        "timeline-check" => {
            // Offline chart consistency report: node/branch/ending counts,
            // roots, dangling jump targets. Costs half a second, no window.
            let dir = args.get(2).map(String::as_str).unwrap_or("data/script");
            let lines = timeline::check(Path::new(dir));
            for line in lines {
                println!("{}", line);
            }
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
        "font-sample" => {
            // font-sample <out.tga> [w] [h]  : fixed vector-font specimen sheet
            // (body, dense glyphs, punctuation, Latin mix, and the real dialog
            // path with a name and three lines), drawn with the WA2_FONT*
            // settings in force so two configurations can be compared.
            if args.len() < 3 { eprintln!("usage: wa2-native font-sample <out.tga> [w] [h]"); return; }
            let out = args[2].clone();
            let w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280);
            let h: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(720);
            let choice = render::text::font_choice();
            let mut vf = match render::text::VectorFont::open_face(&choice.path, render::DIALOG_FONT_720, choice.face) {
                Some(f) => f,
                None => { eprintln!("font-sample: cannot open {} (face {})", choice.path, choice.face); return; }
            };
            render::text::apply_choice(&vf, &choice);
            eprintln!("font: {}", vf.describe());
            let mut s = render::Surface::new(w, h, [20, 20, 28, 255]);
            let px = |v: i32| (v * h as i32 + 360) / 720;
            let size = px(render::DIALOG_FONT_720 as i32).max(8) as u32;
            let _ = vf.set_size(size);
            let body = render::text::TextStyle::body();
            let label = render::text::TextStyle::name();
            let mut y = px(40);
            let step = px(56);
            vf.draw_styled(&mut s, &vf.describe(), px(40), y, &label);
            y += step;
            for (tag, text) in [
                ("正文", "春希、雪菜、冬马和纱，路边的树木叶子差不多已经落尽"),
                ("复杂", "鬱邊體樂觀發臺裏骨直着国门广发"),
                ("标点", "，。！？「」『』\u{201c}\u{201d}\u{2018}\u{2019}……——"),
                ("混排", "WHITE ALBUM 2 100% 2026"),
            ] {
                vf.draw_styled(&mut s, tag, px(40), y, &label);
                vf.draw_styled(&mut s, text, px(140), y, &body);
                y += step;
            }
            // The dialog itself, through the same path the game uses: a name on
            // the rim and three body lines in the oval.
            let dialog = "已经，什么都无所谓了。\n无论是白色季节里的雪色，\n还是渐渐残碎的约定。";
            render::draw_dialog_vector(&mut s, &mut vf, dialog, Some("冬马和纱"), w as i32, h as i32, usize::MAX);
            if let Err(e) = s.save_tga(&out) { eprintln!("error writing {}: {}", out, e); std::process::exit(1); }
            println!("font-sample: {}x{} -> {}", w, h, out);
        }
        "menu-clean" => {
            // menu-clean: bake the caption-free option pages once, so the
            // settings screen stops repairing pixels on every state change.
            let Some(d) = data::Data::resolve() else { return };
            // Same search order the settings screen uses: the Chinese overlay
            // sheets first, then the original. The 1x trees come first on
            // purpose - the menu draws at logical size and the 4x upscaler
            // mangled some of its sheets.
            let mut dirs_buf: Vec<std::path::PathBuf> = Vec::new();
            for name in ["bg_cn", "bg", "bg_cn_8x", "bg_8x"] {
                let p = d.root.join(name);
                if p.is_dir() {
                    dirs_buf.push(p);
                }
            }
            let dirs: Vec<&std::path::Path> = dirs_buf.iter().map(|p| p.as_path()).collect();
            let out = std::path::Path::new(&std::env::var("WA2_DATA").unwrap_or_else(|_| "data".into()))
                .join("cache/ui/menu");
            let pages = menu::bake_clean_pages(&dirs, &out);
            eprintln!("menu-clean: {} page(s) -> {}", pages.len(), out.display());
            println!("wrote {} clean page(s) to {}", pages.len(), out.display());
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
                render::render_scene_ex(&bg, &page.text(), &font, 1280, 720, &[], lut.as_deref(), true, None, None, 255, 0, &[]);
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
            let surf = render::render_scene_sysfont(&bg, &page.text(), 1280, 720);
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
                let surf = render::render_scene_sysfont(&bg, &page.lines[idx.max(0) as usize], 1280, 720);
                win.present(&surf);
                loop {
                    match win.poll_input() {
                        sdl2::Input::Advance | sdl2::Input::Right => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Back => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Up => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Down => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::RightClick => break,
                        sdl2::Input::Drag { .. } => {}
                        sdl2::Input::None => win.delay(16),
                        sdl2::Input::Choice(_) => win.delay(16),
                        sdl2::Input::Pointer { .. } | sdl2::Input::Release { .. } => win.delay(16),
                        sdl2::Input::Wheel { y } if y < 0 => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Wheel { .. } => win.delay(16),
                        sdl2::Input::Rewind => win.delay(16),
                        sdl2::Input::Nav(_) => win.delay(16),
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
                        sdl2::Input::Advance | sdl2::Input::Right => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Back => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Up => { if idx > 0 { idx -= 1; } break; }
                        sdl2::Input::Down => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Quit => return,
                        sdl2::Input::Fullscreen => break,
                        sdl2::Input::RightClick => break,
                        sdl2::Input::Drag { .. } => {}
                        sdl2::Input::Choice(_) => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Pointer { pressed, .. } => { if pressed && idx < n - 1 { idx += 1; break; } win.delay(16); }
                        sdl2::Input::Release { .. } => win.delay(16),
                        sdl2::Input::None => { win.delay(16); }
                        sdl2::Input::Wheel { y } if y < 0 => { if idx < n - 1 { idx += 1; } break; }
                        sdl2::Input::Wheel { .. } => win.delay(16),
                        sdl2::Input::Rewind => win.delay(16),
                        sdl2::Input::Nav(_) => win.delay(16),
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
                    "wa2-native run: data={} start={} (Space/Click=next, Up/Wheel=rewind, PageUp=fast rewind, 1-9=choice, Esc/q=quit)",
                    d.describe(), start_id
                );
                let session = Session::from_data(&d, None);
                let mut rt = Runtime::new(&win, &session);
                if let Err(e) = run_game(&mut win, &font, &session, &start_id, &mut rt) {
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
                cn: is_cn_script(&script_dir),
                bg_trees: vec![(1, bg_dir.clone())],
                bg_cn_trees: Vec::new(),
                bg_cn_original: None,
                bg_original: None,
                sprite_trees: find_sprite_dir(&bg_dir, &script_dir)
                    .map(|d| vec![(1, d)])
                    .unwrap_or_default(),
                movie_dir: bg_dir.join("..").join("mv"),
                voice_dir,
                bgm: bgm_path,
                bg_index: BgIndex::build(&[&bg_dir]),
            };
            eprintln!("wa2-native run: start={} (Space/Click=next, Left=back, 1-9=choice, Esc/q=quit)", start_id);
            let mut rt = Runtime::new(&win, &session);
            if let Err(e) = run_game(&mut win, &font, &session, &start_id, &mut rt) {
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
                    cn: is_cn_script(&script_dir),
                    bg_trees: vec![(1, bg_dir.clone())],
                    bg_cn_trees: Vec::new(),
                    bg_cn_original: None,
                    bg_original: None,
                    sprite_trees: find_sprite_dir(&bg_dir, &script_dir)
                        .map(|d| vec![(1, d)])
                        .unwrap_or_default(),
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
            let mut rt = Runtime::new(&win, &session);
            match run_title(&mut win, &font, &session.bg_dirs(), &session.script_dir, !session.bg_cn_trees.is_empty()) {
                TitleAction::Start => {
                    eprintln!("title: starting scene {}", start_id);
                    if let Err(e) = run_game(&mut win, &font, &session, &start_id, &mut rt) {
                        eprintln!("run error: {}", e);
                    }
                }
                TitleAction::Jump(id) => {
                    eprintln!("title: timeline jump {}", id);
                    if let Err(e) = run_game(&mut win, &font, &session, &id, &mut rt) {
                        eprintln!("run error: {}", e);
                    }
                }
                TitleAction::Load(snap) => {
                    eprintln!("title: loading scene {}", snap.scene);
                    if let Err(e) = run_game_from(&mut win, &font, &session, snap, &mut rt) {
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
                let prog = match load_program(dir, &id, is_cn_script(dir)) {
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
                        vm::lscr::Event::EventCg { id, variant, frames } =>
                            println!("  [cg  ] id={} var={} frames={}", id, variant, frames),
                        vm::lscr::Event::Flash { frames, r, g, b } =>
                            println!("  [flash] {},{},{} {}f", r, g, b, frames),
                        vm::lscr::Event::Overlay { name, slot } => println!("  [ovl ] slot={} {}", slot, name),
                        vm::lscr::Event::WaitFrames(n) => println!("  [wait] {} frames", n),
                        vm::lscr::Event::Sound { opcode, args } =>
                            println!("  [sound] ACT{} {:?}", opcode, args),
                        vm::lscr::Event::Tint { name } => println!("  [tint] {}", if name.is_empty() { "<none>" } else { &name }),
                        vm::lscr::Event::Movie { name } => println!("  [mv  ] {}", name),
                        vm::lscr::Event::Sprite { char_id, series, variant, slot, frames } =>
                            println!("  [spr ] char={} series={} variant={} slot={} frames={}", char_id, series, variant, slot, frames),
                        vm::lscr::Event::SpriteExit { char_id, frames } =>
                            println!("  [spr-] char={} frames={}", char_id, frames),
                        vm::lscr::Event::SpriteFocus { char_id } => println!("  [fo-c] {}", char_id),
                        vm::lscr::Event::Window { mode } => println!("  [win ] mode={}", mode),
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
            let steps = vm::parse_steps(&scen, is_cn_script(std::path::Path::new(&args[2]).parent().unwrap_or(std::path::Path::new("."))));
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
/// ACT148 / ACT201 file. The exe's mode-1 sprintf is `v%06d.tga` of
/// `id * 10 + variant`: 10170 variant 0 is `v101700.tga`.
fn event_cg_name(id: i64, variant: u32) -> String {
    format!("v{:06}.tga", id * 10 + variant as i64)
}

/// Scene-switch profiling. `WA2_TIMING=1` prints the cost of each decode,
/// composite and upload so a stall can be attributed to one of them.
fn timing_enabled() -> bool {
    std::env::var_os("WA2_TIMING").is_some()
}

fn tlog(label: &str, t0: std::time::Instant) {
    if timing_enabled() {
        eprintln!("  [timing] {label}: {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
    }
}

fn source_art_scale() -> u32 {
    std::env::var("WA2_ART_SCALE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| matches!(v, 1 | 2 | 4 | 8))
        .unwrap_or(1)
}

/// How the decode scale is chosen. Decided once, before any call writes a
/// number into `WA2_RENDER_SCALE`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScaleMode {
    /// `WA2_RENDER_SCALE=auto`: decode only as much as the window can show.
    Window,
    /// Numeric `WA2_RENDER_SCALE`: exactly that scale.
    Forced(u32),
    /// No override: never below 4x, more only when the window needs it.
    Default,
}

fn scale_mode() -> ScaleMode {
    static MODE: std::sync::OnceLock<ScaleMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("WA2_RENDER_SCALE") {
        Ok(v) if v.eq_ignore_ascii_case("auto") => ScaleMode::Window,
        Ok(v) => match v.trim().parse::<u32>() {
            Ok(n) if matches!(n, 1 | 2 | 4 | 8) => ScaleMode::Forced(n),
            _ => ScaleMode::Default,
        },
        Err(_) => ScaleMode::Default,
    })
}

/// Decode scale wanted under the current mode.
///
/// Default: windowed decodes at 4x (the 4x tree is read directly), fullscreen
/// decodes at 8x (the master tree, even on a 1080p panel where the extra
/// pixels are supersampling). `WA2_RENDER_SCALE=auto` keeps the old
/// window-sized decoding, a number forces one scale.
fn decode_scale_for_window(win: &sdl2::Sdl) -> u32 {
    let (w, h) = win.window_size();
    match scale_mode() {
        ScaleMode::Forced(n) => n,
        ScaleMode::Window => render_scale_for(w, h),
        ScaleMode::Default => {
            if win.is_fullscreen() || win.is_maximized() {
                8
            } else {
                render_scale_for(w, h).max(4)
            }
        }
    }
}

/// How many pixels one decoded art asset stores per 1280x720 layout pixel.
fn render_art_scale() -> u32 {
    if let ScaleMode::Forced(n) = scale_mode() {
        return n;
    }
    std::env::var("WA2_RENDER_SCALE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|&v| matches!(v, 1 | 2 | 4 | 8))
        .unwrap_or_else(source_art_scale)
}

/// Report and apply the decode scale for the current window.
fn set_render_art_scale(win: &sdl2::Sdl) {
    let (w, h) = win.window_size();
    let want = decode_scale_for_window(win);
    static REPORTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    REPORTED.get_or_init(|| {
        let why = match scale_mode() {
            ScaleMode::Forced(n) => format!("forced {}x by WA2_RENDER_SCALE", n),
            ScaleMode::Window => "auto: window-sized decoding".to_string(),
            ScaleMode::Default => {
                "default: windowed 4x / fullscreen or maximized 8x; WA2_RENDER_SCALE=auto for window-sized decoding"
                    .to_string()
            }
        };
        eprintln!(
            "art: source {}x, decode {}x for window {}x{} ({})",
            source_art_scale(),
            want,
            w,
            h,
            why
        );
    });
    let current = std::env::var("WA2_RENDER_SCALE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok());
    if current != Some(want) {
        std::env::set_var("WA2_RENDER_SCALE", want.to_string());
        // Pictures decoded for the old scale are the wrong pixel size now;
        // dropping them keeps the compositor from mixing scales.
        artcache::drop_other_scales(want);
        eprintln!("art: render scale={} for window {}x{}", want, w, h);
    }
}

/// Largest useful art scale for a window: 1280x720 needs 1x, 1920x1080
/// needs 2x, 2560x1440/4K need 4x, anything larger 8x.
fn render_scale_for(w: i32, h: i32) -> u32 {
    let w = w.max(1280) as u32;
    let h = h.max(720) as u32;
    let need = ((w + 1279) / 1280).max((h + 719) / 720);
    if need <= 1 { 1 } else if need <= 2 { 2 } else if need <= 4 { 4 } else { 8 }
}

fn load_background(dirs: &[&std::path::Path], name: &str) -> Option<codec::tga::TgaImage> {
    load_background_asset(dirs, name).map(|asset| asset.img)
}

/// load_background_asset_inner with a WA2_TIMING profile line.
fn load_background_asset(dirs: &[&std::path::Path], name: &str) -> Option<LoadedAsset> {
    let t0 = std::time::Instant::now();
    let out = load_background_asset_inner(dirs, name);
    tlog(&format!("load {name}"), t0);
    out
}

/// Load a scene asset and retain the scale represented by the decoded pixels.
/// UI sheets are normalized by decode_ui_file, while art keeps the largest
/// useful scale allowed by the current render target.
fn load_background_asset_inner(dirs: &[&std::path::Path], name: &str) -> Option<LoadedAsset> {
    let t0 = std::time::Instant::now();
    let is_ui =
        name.starts_with("sys_") || name.starts_with("SYS_") || name.starts_with('t') || name.starts_with('T')
            || name.eq_ignore_ascii_case("weather.tga");
    let found = resolve_asset_path(dirs, name);
    tlog("resolve", t0);
    let t1 = std::time::Instant::now();
    // UI sheets are consumed at their 1280x720 logical size and the tool
    // pre-renders them under data/cache/ui, so they only need the decoder as a
    // fallback. Scene art goes through the derived cache: an 8x WebP is decoded
    // once and every later load of it is a file read or a memory hit.
    for (path, cp932) in found {
        let decoded = if is_ui {
            codec::image::decode_ui_file(&path).ok()
        } else {
            artcache::load(&path).map(|img| (*img).clone())
        };
        if let Some(img) = decoded {
            let (art_scale, src_scale) = if is_ui {
                (1, 1)
            } else {
                let (src, dec) = codec::image::art_scales(&path);
                (dec.max(1), src.max(1))
            };
            tlog(if cp932 { "decode (cp932 name)" } else { "decode" }, t1);
            return Some(LoadedAsset { img, art_scale, src_scale });
        }
    }
    None
}

/// Find the file backing one logical asset name without decoding anything.
///
/// Candidate names are tried against every tree with a plain existence check,
/// which is what the extracted data tree answers directly. Only when nothing
/// matches does it fall back to the CP932 name scan needed by trees extracted
/// before the extractor normalized filenames - and that scan is indexed once
/// per directory, because reading 1200 directory entries per candidate was
/// measurably slower than decoding the image afterwards.
///
/// Returns every existing file the name could mean, best first, each flagged
/// with whether it came from the CP932 fallback. The caller decodes them in
/// order, so a file that fails to decode still falls through to the next
/// candidate - which is what the old decode-as-you-search loop did.
fn resolve_asset_path(dirs: &[&std::path::Path], name: &str) -> Vec<(std::path::PathBuf, bool)> {
    let candidates = asset_candidates(name);
    let mut out = Vec::new();
    for dir in dirs {
        for cand in &candidates {
            let p = dir.join(cand);
            if p.is_file() {
                out.push((p, false));
            }
        }
    }
    #[cfg(unix)]
    {
        for dir in dirs {
            for (decoded, path) in cp932_entries(dir).iter() {
                for cand in &candidates {
                    if decoded.eq_ignore_ascii_case(cand) {
                        out.push((path.clone(), true));
                    }
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = dirs;
    out
}

/// The names one logical asset may be stored under.
fn asset_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_string()];
    let lower = name.to_ascii_lowercase();
    for sfx in ["_bg", "_chr", "_chreff", "_ev", "_fg", "_eff"] {
        if lower.ends_with(sfx) {
            candidates.push(name[..name.len() - sfx.len()].to_string());
        }
    }
    // Each sibling tree keeps the same stem but may use WebP after upscale.
    // .bmp sources (backgrounds extracted from the archive) keep the same
    // stem in the WebP trees as well.
    let base = name
        .trim_end_matches(".tga")
        .trim_end_matches(".TGA")
        .trim_end_matches(".webp")
        .trim_end_matches(".WEBP")
        .trim_end_matches(".bmp")
        .trim_end_matches(".BMP");
    candidates.push(format!("{}.webp", base));
    candidates.push(format!("{}.WEBP", base));
    candidates.push(format!("{}.tga", base));
    candidates.push(format!("{}.TGA", base));
    candidates
}

/// Directory entries with their names CP932-decoded, indexed once per directory.
#[cfg(unix)]
fn cp932_entries(dir: &std::path::Path) -> std::sync::Arc<Vec<(String, std::path::PathBuf)>> {
    use std::collections::HashMap;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::{Arc, Mutex, OnceLock};
    static IDX: OnceLock<Mutex<HashMap<std::path::PathBuf, Arc<Vec<(String, std::path::PathBuf)>>>>> =
        OnceLock::new();
    let mut m = match IDX.get_or_init(Default::default).lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    if let Some(v) = m.get(dir) {
        return Arc::clone(v);
    }
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let raw = entry.file_name();
            v.push((codec::sjis::decode(raw.as_bytes()), entry.path()));
        }
    }
    let v = Arc::new(v);
    m.insert(dir.to_path_buf(), Arc::clone(&v));
    v
}
/// Exact voice lookup by the identifiers from ACT137 and ACT138.
struct VoiceIndex {
    map: std::collections::HashMap<(u32, u32, u32), std::path::PathBuf>,
}

impl VoiceIndex {
    fn build(dir: &std::path::Path) -> VoiceIndex {
        let t0 = std::time::Instant::now();
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
        tlog("voice index", t0);
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
                let picture = (lower.starts_with('b') || lower.starts_with('v'))
                    && (lower.ends_with(".tga") || lower.ends_with(".webp"));
                if !picture {
                    continue;
                }
                let ext_len = if lower.ends_with(".webp") { 5 } else { 4 };
                let digits = lower[1..lower.len() - ext_len].to_string();
                if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let id: u32 = digits[..4].parse().unwrap_or(0);
                let incoming_b = lower.starts_with('b');
                by_name.entry(lower).or_insert_with(|| name.clone());
                match by_id.entry(id) {
                    Entry::Vacant(v) => {
                        v.insert(name.clone());
                    }
                    Entry::Occupied(mut o) => {
                        let held_b = o
                            .get()
                            .as_bytes()
                            .first()
                            .is_some_and(|c| *c == b'b' || *c == b'B');
                        // `b*` is the background. A `v*` CG fills the id only
                        // when no background file was shipped for it.
                        if incoming_b && (digits.ends_with("00") || !held_b) {
                            o.insert(name.clone());
                        } else if !incoming_b && !held_b && digits.ends_with("00") {
                            o.insert(name.clone());
                        }
                    }
                }
            }
        }
        BgIndex { by_name, by_id }
    }

    /// `b{id:04}{variant:02}.tga`, then the same number as a CG (`v*`),
    /// else any file of that id.
    ///
    /// Event CGs (ACT148) do not use this. Their file is `v{id*10+variant:06}`.
    fn resolve(&self, id: i64, variant: u32) -> Option<String> {
        if id <= 0 {
            return None;
        }
        for prefix in ["b", "v"] {
            for ext in [".webp", ".tga"] {
                let exact = format!("{}{:04}{:02}{}", prefix, id, variant, ext);
                if let Some(n) = self.by_name.get(&exact) {
                    return Some(n.clone());
                }
            }
        }
        self.by_id.get(&(id as u32)).cloned()
    }
}

/// Pictures a scenario will ask for later, in program order.
///
/// Scanning the bytecode is cheap and lets the prefetch thread decode the next
/// backgrounds and sprites while the player is still reading the line on
/// screen, which is what removes the wait at a scene change. The scan mirrors
/// the interpreter record handling: K30 with three ints followed by a K16
/// ACT154/155 is a sprite, ACT146/147 a background, ACT148/201 a CG.
fn upcoming_art(
    p: &vm::lscr::Program,
    bg: &BgIndex,
    bank: Option<&SpriteBank>,
) -> (Vec<String>, Vec<String>) {
    use vm::lscr::Op;
    let mut bgs: Vec<String> = Vec::new();
    let mut sprites: Vec<String> = Vec::new();
    let mut ints: Vec<i32> = Vec::new();
    let (mut kind, mut tok_hit, mut act_hit) = (0u32, false, false);
    let mut pending: Option<(u32, u32, u32)> = None;
    for (i, op) in p.ops.iter().enumerate() {
        match op {
            Op::Rec { kind: k, .. } => {
                if kind == 30 && !tok_hit && !act_hit && ints.len() == 3 {
                    let cid = ints[0];
                    let variant = ints[1];
                    let flag = ints[2];
                    if (0..40).contains(&cid)
                        && (0..1000).contains(&variant)
                        && flag % 1000 == 0
                        && (1000..=30000).contains(&flag)
                        && p.ops[i..]
                            .iter()
                            .take(10)
                            .any(|o| matches!(o, Op::Act { id, .. } if *id == 154 || *id == 155))
                    {
                        pending = Some((cid as u32, (flag / 1000) as u32, variant as u32));
                    }
                }
                ints.clear();
                kind = *k;
                tok_hit = false;
                act_hit = false;
            }
            Op::Tok { .. } => tok_hit = true,
            Op::Int { v, .. } => ints.push(*v),
            Op::Act { id, .. } => {
                act_hit = true;
                match *id {
                    146 | 147 => {
                        if ints.len() >= 4 && ints[1] > 0 {
                            if let Some(n) = bg.resolve(ints[1] as i64, ints[2].max(0) as u32) {
                                if !bgs.contains(&n) {
                                    bgs.push(n);
                                }
                            }
                        }
                    }
                    148 => {
                        if ints.len() >= 4 && ints[1] > 0 {
                            let n = event_cg_name(ints[1] as i64, ints[2].max(0) as u32);
                            if !bgs.contains(&n) {
                                bgs.push(n);
                            }
                        }
                    }
                    201 => {
                        if ints.len() >= 3 && ints[0] > 0 && ints[2] != 0 {
                            let n = event_cg_name(ints[0] as i64, ints[1].max(0) as u32);
                            if !bgs.contains(&n) {
                                bgs.push(n);
                            }
                        }
                    }
                    154 | 155 => {
                        if let (Some((cid, series, variant)), Some(bank)) = (pending.take(), bank) {
                            if let Some(n) = bank.resolve(cid, series, variant) {
                                if !sprites.contains(&n) {
                                    sprites.push(n);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    (bgs, sprites)
}

/// How many pictures of each kind to queue ahead of the script.
const PREFETCH_BG: usize = 6;
const PREFETCH_SPRITE: usize = 8;

/// Turn the names the script will ask for into file paths for the prefetch
/// thread. Missing files are dropped here rather than in the worker.
fn prefetch_paths(
    dirs: &[&std::path::Path],
    bgs: &[String],
    sprite_dir: Option<&std::path::Path>,
    sprites: &[String],
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for name in bgs.iter().take(PREFETCH_BG) {
        if let Some((p, _)) = resolve_asset_path(dirs, name).into_iter().next() {
            out.push(p);
        }
    }
    if let Some(dir) = sprite_dir {
        for name in sprites.iter().take(PREFETCH_SPRITE) {
            let p = dir.join(name);
            if p.is_file() {
                out.push(p);
            }
        }
    }
    out
}

/// Where every kind of media comes from for one play session.
///
/// Built either from the loose-file data tree (`Session::from_data`) or from
/// explicit CLI paths (legacy debugging form). Everything the engine loads at
/// runtime goes through here - no `.pak` archive is opened.
struct Session {
    script_dir: std::path::PathBuf,
    /// True when `script_dir` is the CK-GAL Chinese patch: its text needs the
    /// glyph-table reading (see `codec::sjis::decode_cn`).
    cn: bool,
    /// Scaled art trees, `(scale, dir)` ascending. Both the 4x tree and the
    /// 8x master are kept when present: the decode scale decides which one is
    /// read first, so entering fullscreen switches to the 8x master without
    /// rebuilding the session.
    bg_trees: Vec<(u32, std::path::PathBuf)>,
    /// Chinese-patch UI overrides - the 31 images shipped loose in the game's
    /// `grp/` directory, which win over the same-named files inside `grp.pak`.
    /// Consulted before `bg_trees`; empty in Japanese mode.
    bg_cn_trees: Vec<(u32, std::path::PathBuf)>,
    /// Original Chinese UI tree used as a per-file fallback while the
    /// optional high-resolution sibling is still being generated.
    bg_cn_original: Option<std::path::PathBuf>,
    /// Original background tree used when the selected high-resolution file
    /// is not present yet.
    bg_original: Option<std::path::PathBuf>,
    /// Scaled sprite trees, ascending; empty disables sprites.
    sprite_trees: Vec<(u32, std::path::PathBuf)>,
    /// Movies as plain `*.wmv` / `*.asf` files.
    movie_dir: std::path::PathBuf,
    voice_dir: Option<std::path::PathBuf>,
    bgm: Option<String>,
    /// ACT146/147 background id -> file index.
    bg_index: BgIndex,
}

impl Session {
    fn from_data(d: &data::Data, bgm: Option<String>) -> Session {
        // Keep the scaled sibling trees, not just the one Data picked: the
        // decode scale changes at runtime (fullscreen asks for the 8x master)
        // and the loader then just reorders these directories.
        let mut bg_trees = vec![(d.art_scale, d.bg())];
        let mut sprite_trees = vec![(d.art_scale, d.sprite())];
        let mut bg_cn_trees = if d.cn {
            vec![(d.art_scale, d.bg_cn())]
        } else {
            Vec::new()
        };
        // The default source is the 4x tree; keep the 8x master next to it so
        // fullscreen can read it. Trees the player chose explicitly (2x proxy,
        // original TGA) are left alone.
        if d.art_scale == 4 {
            let bg8 = d.root.join("bg_8x");
            let sp8 = d.root.join("sprite_8x");
            let cn8 = d.root.join("bg_cn_8x");
            if bg8.is_dir() {
                bg_trees.push((8, bg8));
                if sp8.is_dir() {
                    sprite_trees.push((8, sp8));
                }
                if d.cn && cn8.is_dir() {
                    bg_cn_trees.push((8, cn8));
                }
            }
        }
        bg_trees.sort_by_key(|(scale, _)| *scale);
        sprite_trees.sort_by_key(|(scale, _)| *scale);
        bg_cn_trees.sort_by_key(|(scale, _)| *scale);
        let bg_original = d.root.join("bg");
        let bg_cn_original = if d.cn { Some(d.root.join("bg_cn")) } else { None };
        let mut index_dirs: Vec<&std::path::Path> = Vec::new();
        for (_, p) in &bg_cn_trees {
            index_dirs.push(p.as_path());
        }
        if let Some(ref p) = bg_cn_original {
            index_dirs.push(p.as_path());
        }
        for (_, p) in &bg_trees {
            index_dirs.push(p.as_path());
        }
        index_dirs.push(bg_original.as_path());
        let bg_index = BgIndex::build(&index_dirs);
        Session {
            script_dir: d.script(),
            cn: d.cn,
            bg_trees,
            bg_cn_trees,
            bg_cn_original,
            bg_original: Some(bg_original),
            sprite_trees,
            movie_dir: d.movie(),
            voice_dir: Some(d.voice()),
            bgm,
            bg_index,
        }
    }

    /// Directories searched for images, in priority order.
    ///
    /// The scaled trees are ordered for the current decode scale: a 4x decode
    /// reads the 4x tree first (about four times faster than shrinking the 8x
    /// file), an 8x decode reads the 8x master. The 1280x720 originals stay as
    /// the last fallback for files the scaled trees are missing.
    fn bg_dirs(&self) -> Vec<&std::path::Path> {
        let want = render_art_scale();
        let mut v = Vec::new();
        v.extend(ordered_tree_paths(&self.bg_cn_trees, want));
        v.extend(ordered_tree_paths(&self.bg_trees, want));
        if let Some(cn) = self.bg_cn_original.as_deref() {
            v.push(cn);
        }
        if let Some(original) = self.bg_original.as_deref() {
            v.push(original);
        }
        v
    }

    /// Sprite directory for the current decode scale, if any.
    fn sprite_dir_now(&self) -> Option<&std::path::Path> {
        ordered_tree_paths(&self.sprite_trees, render_art_scale())
            .into_iter()
            .next()
    }
}

/// Order scaled trees for a decode target: exact match first, then the next
/// larger, then the smaller ones.
fn ordered_tree_paths<'a>(
    trees: &'a [(u32, std::path::PathBuf)],
    want: u32,
) -> Vec<&'a std::path::Path> {
    let mut order: Vec<usize> = (0..trees.len()).collect();
    order.sort_by_key(|&i| {
        let scale = trees[i].0;
        if scale == want {
            0
        } else if scale > want {
            1 + (scale - want)
        } else {
            100 + (want - scale)
        }
    });
    order.into_iter().map(|i| trees[i].1.as_path()).collect()
}

#[cfg(test)]
mod art_tree_tests {
    use super::ordered_tree_paths;
    use std::path::PathBuf;

    #[test]
    fn ordered_trees_prefer_the_decode_scale() {
        let trees = vec![(4u32, PathBuf::from("bg_4x")), (8, PathBuf::from("bg_8x"))];
        let names = |want: u32| -> Vec<String> {
            ordered_tree_paths(&trees, want)
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        // Windowed decodes at 4x and must read the 4x tree first.
        assert_eq!(names(4), ["bg_4x", "bg_8x"]);
        // Fullscreen decodes at 8x and must read the master first.
        assert_eq!(names(8), ["bg_8x", "bg_4x"]);
        // A lower target takes the next larger tree, then the smaller one.
        assert_eq!(names(2), ["bg_4x", "bg_8x"]);
    }
}

#[cfg(test)]
mod sprite_bank_tests {
    use super::SpriteBank;

    /// The bank is built for the windowed tree and must move to the 8x tree
    /// when the decode scale grows (fullscreen), dropping the old pixels.
    #[test]
    fn sprite_bank_retargets_between_scaled_trees() {
        let tmp = std::env::temp_dir().join(format!("wa2-spritebank-{}", std::process::id()));
        let four = tmp.join("sprite_4x");
        let eight = tmp.join("sprite_8x");
        std::fs::create_dir_all(&four).unwrap();
        std::fs::create_dir_all(&eight).unwrap();
        std::fs::write(four.join("a.tga"), b"x").unwrap();
        std::fs::write(eight.join("b.webp"), b"x").unwrap();
        let mut bank = SpriteBank::new(vec![(4, four.clone()), (8, eight.clone())]);
        bank.retarget(4);
        assert_eq!(bank.dir, four);
        assert!(bank.index.contains("a.tga"));
        bank.retarget(8);
        assert_eq!(bank.dir, eight);
        assert!(bank.index.contains("b.webp"));
        assert!(bank.cache.is_empty(), "sprites from the old scale must be dropped");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

/// Everything that outlives a single scene.
///
/// Quick-loading a save used to rebuild all of this inside run_game_at: the
/// audio device was closed and reopened, the 46k-clip voice directory was
/// scanned again, and the UI sheets were decoded again. Keeping them here means
/// a scene change only replaces what the scene actually owns.
pub struct Runtime {
    /// Vector font for Chinese dialogue (None in Japanese mode).
    vfont: Option<render::text::VectorFont>,
    bar: cmdbar::CmdBar,
    msg_plate: Option<codec::tga::TgaImage>,
    msg_frame: Option<codec::tga::TgaImage>,
    voices: Option<VoiceIndex>,
    mixer: Option<audio::Mixer>,
    bank: Option<SpriteBank>,
}

impl Runtime {
    pub fn new(win: &sdl2::Sdl, s: &Session) -> Runtime {
        let choice = render::text::font_choice();
        let mut vfont = if s.cn {
            render::text::VectorFont::open_face(&choice.path, render::DIALOG_FONT_720, choice.face)
        } else {
            None
        };
        if let Some(vf) = vfont.as_ref() {
            render::text::apply_choice(vf, &choice);
            // Which face of the .ttc actually opened, and with what knobs: the
            // file name alone does not say, and a .ttc defaults to its first
            // (Japanese) face.
            eprintln!("font: {}", vf.describe());
        }
        if s.cn && vfont.is_none() {
            eprintln!("dialog: no vector font, falling back to the bitmap atlas");
        }
        let bar = cmdbar::CmdBar::load(&s.bg_dirs());
        let msg_plate = load_background(&s.bg_dirs(), "sys_00001.tga");
        let msg_frame = load_background(&s.bg_dirs(), "sys_00000.tga");
        let voices = s.voice_dir.as_deref().map(VoiceIndex::build);
        let wants_audio = s.bgm.is_some()
            || s.voice_dir.is_some()
            || s.script_dir
                .parent()
                .map(|p| p.join("bgm").is_dir() || p.join("se").is_dir())
                .unwrap_or(false);
        let mixer = if wants_audio {
            match audio::Mixer::new(win) {
                Some(mx) => {
                    if let Some(bgm) = s.bgm.as_deref() {
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
        let bank = if s.sprite_trees.is_empty() {
            None
        } else {
            for (scale, dir) in &s.sprite_trees {
                if dir.is_dir() {
                    eprintln!("sprites: {} ({}x)", dir.display(), scale);
                }
            }
            Some(SpriteBank::new(s.sprite_trees.clone()))
        };
        if bank.is_none() {
            eprintln!("note: no sprite directory - character sprites disabled");
        }
        Runtime { vfont, bar, msg_plate, msg_frame, voices, mixer, bank }
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
    rt: &mut Runtime,
) -> Result<(), String> {
    // Loop so that loading a save slot from the command bar restarts the run
    // at the loaded position instead of ending the game.
    let mut resume: Option<save::SaveState> = None;
    loop {
        match run_game_at(win, font, s, resume, start_id, rt)? {
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
    rt: &mut Runtime,
) -> Result<(), String> {
    let mut resume = Some(snap);
    let scene = resume.as_ref().unwrap().scene.clone();
    loop {
        match run_game_at(win, font, s, resume, &scene, rt)? {
            None => return Ok(()),
            Some(st) => resume = Some(st),
        }
    }
}
fn run_game_at(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    s: &Session,
    resume: Option<save::SaveState>,
    start_id: &str,
    rt: &mut Runtime,
) -> Result<Option<save::SaveState>, String> {
    set_render_art_scale(win);
    let mut resume = resume;
    let script_dir = s.script_dir.as_path();
    const W: u32 = 1280;
    const H: u32 = 720;
    // The art is laid out at 1280x720 and stretched to the window; the
    // dialogue is rasterized at the window's own size so it stays sharp.
    let mut win_size = win.window_size();
    // The vector font, the command bar, the message-window sheets, the voice
    // index, the mixer and the sprite bank all live in the Runtime, so a
    // quick load replaces the scene state instead of the whole engine.
    let vfont = &mut rt.vfont;
    let bar = &rt.bar;
    let msg_plate = &rt.msg_plate;
    let msg_frame = &rt.msg_frame;
    let mut cmd_auto = false;
    let mut cmd_skip = false;
    // Lines already shown, for the 履历 (backlog) screen.
    let mut backlog: Vec<String> = Vec::new();
    // One snapshot per line the player has been shown, newest last. Rewind
    // pops the line on screen and puts the previous one back, picture included.
    let mut history: Vec<LineSnap> = Vec::new();
    let mut rewind_left: i32 = 0;
    let mut cmd_rewind = false;
    let mut line_voice: Option<vm::lscr::VoiceCue> = None;
    // Set when rewind restored a line whose script is not the one loaded now.
    let mut park_wait = false;
    let mut rewind_vm: Option<vm::lscr::InterpState> = None;
    let mut current_id = start_id.to_string();
    // Black until the script sets one: WA2 opens on a black screen (the IC
    // prologue's "sepia.AMP" is a color-filter LUT, not a background image).
    let mut bg = solid_bg(W, H, [0, 0, 0]);
    // Decode scale of the pixels in `bg`; `None` while the screen is a solid
    // colour. A fullscreen switch changes the wanted scale, which tells the
    // loop to re-decode the picture at the new one.
    let mut bg_scale: Option<u32> = None;
    // Source tree the background was read from (4/8 for bg_4x/bg_8x, 1 for the
    // original TGA tree). Printed with the scene line.
    let mut bg_src_scale: Option<u32> = None;

    let mixer = &rt.mixer;

    // WA2_AUTO       : advance text *and* pick choices automatically (unattended runs)
    // WA2_AUTO_TEXT  : advance text only, stop at choices (interactive verification)
    let auto_pick = std::env::var_os("WA2_AUTO").is_some();
    let auto_text = auto_pick || std::env::var_os("WA2_AUTO_TEXT").is_some();

    // Indexed once by the Runtime: the directory holds 46k clips.
    let voices = &rt.voices;
    // Player settings (option screen / wa2.ini).
    let mut settings = menu::Settings::load();
    // A debug jump (`run <scene>`) starts from the saved 画面模式 too, so the
    // decode scale matches what the title screen would have chosen.
    win.set_fullscreen(settings.win_disp == 1);
    set_render_art_scale(win);
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
    let mut bank = &mut rt.bank;
    // Standing sprites live in numbered stage slots (K16's first value): slot 0
    // left, 1 centre, 2 right. Slots are reused as characters move around.
    let mut slots: [Option<ActiveSprite>; SPRITE_SLOTS] = Default::default();
    // Active `.AMP` colour curve (sepia flashbacks, negative, ...).
    let mut tint: Option<Vec<u8>> = None;
    let mut focus: Option<u32> = None;
    // 「ウィンドウ一時消去」. The button and the script (ACT170) are separate:
    // fast-forward must not light the button or fold the box the player left open.
    let mut user_hide = false;
    let mut script_open = true;
    let mut window = true;
    let mut line_no: u32 = 0;
    // ACT176 plates living on top of the background until the next scene.
    let mut plates: Vec<Plate> = Vec::new();
    // True when the current background is a CG (`v*`), which picks the CG
    // window opacity and skips the novel-mode dim.
    let mut bg_cg = false;
    // Lines this install has already shown. Skip mode consults it when
    // 「未读文本」 is selected.
    let mut seen = load_seen();
    // Typewriter: `line_base` is the frame with the window and the name but
    // no body. Each tick paints a longer prefix onto a copy of it.
    let mut line_base: Option<render::Surface> = None;
    let mut line_done = true;
    let mut reveal: usize = 0;
    let mut type_tick: u32 = 0;
    // Only these three are extra bookkeeping for save/load. The dialog box is
    // not state - `cur_surf` holds the composited pixels but not the string -
    // and the decoded background/tint cannot be stored, so their ids and names
    // are kept next to them and re-resolved when a save is loaded.
    let mut last_text = String::new();
    let mut last_speaker: Option<String> = None;
    let mut tint_name: Option<String> = None;
    let mut bg_id: Option<(i64, u32)> = None;
    // What the screen is actually showing. Printed with every line so a black
    // frame can be told apart from a missing file.
    let mut bg_label = String::from("BLACK");
    // ---- resume from a save slot ----------------------------------------
    if let Some(snap) = resume.as_ref() {
        current_id = snap.scene.clone();
        bg_id = snap.bg;
        if let Some((id, variant)) = snap.bg {
            if let Some(name) = s.bg_index.resolve(id, variant) {
                if let Some(asset) = load_background_asset(&s.bg_dirs(), &name) {
                    bg_cg = name.starts_with('v') || name.starts_with('V');
                    bg_label = name.clone();
                    eprintln!("scene {} | bg {} id={} var={}", current_id, name, id, variant);
                    bg_scale = Some(asset.art_scale);
                    bg_src_scale = Some(asset.src_scale);
                    bg = asset.img;
                } else {
                    eprintln!(
                        "scene {} | bg id={} var={} file {} FAILED to decode, screen stays {}",
                        current_id, id, variant, name, bg_label
                    );
                }
            } else if let Some(asset) =
                load_background_asset(&s.bg_dirs(), &event_cg_name(id, variant))
            {
                // A save made on an event CG stores the script id (10170),
                // which is not a `b{id:04}{var:02}` background.
                bg_cg = true;
                bg_label = event_cg_name(id, variant);
                eprintln!("scene {} | cg {} id={} var={}", current_id, bg_label, id, variant);
                bg_scale = Some(asset.art_scale);
                    bg_src_scale = Some(asset.src_scale);
                bg = asset.img;
            } else {
                eprintln!(
                    "scene {} | bg MISSING id={} var={}, screen stays {}",
                    current_id, id, variant, bg_label
                );
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
            if let Some(bank) = bank.as_mut() {
                if let Some((img, art_scale, src_scale)) = bank.get(name) {
                    slots[i] = Some(ActiveSprite { char_id: *char_id, name: name.clone(), img, art_scale, src_scale });
                }
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
        cur_surf = draw_scene(&bg, &last_text, last_speaker.as_deref(), font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
        win.present(&cur_surf);
        eprintln!("load: scene {} at pc {}", current_id, snap.vm.pc);
        remember(
            &mut history,
            capture_line(
                &current_id,
                snap.vm.clone(),
                bg_id,
                bg_cg,
                &bg_label,
                &tint_name,
                &slots,
                focus,
                &last_text,
                &last_speaker,
                bgm_track.map(|(id, _)| id),
                script_open,
                &plates,
                None,
            ),
        );
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
        let prog = match load_program(script_dir, &current_id, s.cn) {
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
        // Decode what this scene will ask for while the player reads the
        // first lines, so the script never waits on a 59 MP WebP.
        {
            let (bgs, sprites) = upcoming_art(&prog, &s.bg_index, bank.as_ref());
            let queued = prefetch_paths(&s.bg_dirs(), &bgs, s.sprite_dir_now(), &sprites);
            if !queued.is_empty() {
                eprintln!("prefetch: {} pictures", queued.len());
                artcache::prefetch(queued);
            }
        }
        // The saved position belongs to the first scene only; a later scene
        // change means the player moved on, so it starts from its entry point.
        if let Some(vm) = rewind_vm.take() {
            it.restore(vm);
        } else if let Some(snap) = resume.take() {
            it.restore(snap.vm);
        }
        'events: loop {
        win_size = win.window_size();
        if decode_scale_for_window(win) != render_art_scale() {
            set_render_art_scale(win);
        }
        // A decode-scale change (fullscreen switch, window resize) invalidates
        // the pixels on screen: they were decoded for the old scale. Re-decode
        // the background and the standing sprites at the new one instead of
        // showing the old resolution stretched.
        if bg_scale.is_some_and(|scale| scale != render_art_scale()) {
            let dirs = s.bg_dirs();
            let reload = match bg_id {
                Some((id, variant)) => s.bg_index.resolve(id, variant),
                None if !bg_label.eq_ignore_ascii_case("BLACK") => Some(bg_label.clone()),
                None => None,
            };
            if let Some(name) = reload {
                if let Some(asset) = load_background_asset(&dirs, &name) {
                    bg = asset.img;
                    bg_scale = Some(asset.art_scale);
                    bg_src_scale = Some(asset.src_scale);
                    bg_label = name;
                    eprintln!(
                        "art: reloaded {} at decode scale {}x",
                        bg_label,
                        render_art_scale()
                    );
                }
            }
            for slot in slots.iter_mut().flatten() {
                if let Some(bank) = bank.as_mut() {
                    if let Some((img, art_scale, src_scale)) = bank.get(&slot.name) {
                        slot.img = img;
                        slot.art_scale = art_scale;
                        slot.src_scale = src_scale;
                    }
                }
            }
        }
            let event = if park_wait {
                park_wait = false;
                vm::lscr::Event::Wait
            } else {
                it.next_event()
            };
            match event {
                vm::lscr::Event::Text { text, speaker, voice } => {
                    let body = clean_text(&text);
                    let voice_id = match voice {
                        Some(v) => format!("{}_{:04}_{:02}", v.scenario, v.line, v.char_id),
                        None => "-".to_string(),
                    };
                    let muted = voice
                        .and_then(|c| menu::char_row_of(c.char_id))
                        .map(|row| settings.char_voice[row] == 0)
                        .unwrap_or(false);
                    let cue = voice.filter(|_| !muted);
                    line_voice = cue;
                    // 「未读文本」: an unseen line drops out of skip and is shown.
                    let seen_key = format!("{}\t{}", current_id, body.replace('\n', " "));
                    if cmd_skip && settings.text_skip == 1 && !seen.contains(&seen_key) {
                        cmd_skip = false;
                    }
                    if cmd_skip {
                        // Starting a clip per skipped line piles ffmpeg jobs up
                        // across a scene change, and the one still audible is
                        // no longer the line on screen.
                        skipped_voice = cue;
                        if let Some(mx) = mixer.as_ref() {
                            stop_voice(mx, &mut voice_stream);
                        }
                    }
                    last_speaker = speaker.clone();
                    last_text = body.clone();
                    // A spoken line opens the box again. ACT170(0) only hides
                    // it across the scene change that came before this line.
                    if !user_hide {
                        script_open = true;
                    }
                    window = dialog_open(user_hide, script_open, cmd_skip);
                    line_no += 1;
                    let cast = slots
                        .iter()
                        .enumerate()
                        .filter_map(|(i, s)| {
                            s.as_ref().map(|a| {
                                format!("{}:{}({}x)", i, a.name, a.src_scale)
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let cast = if cast.is_empty() {
                        "-".to_string()
                    } else {
                        cast
                    };
                    let focus_id = focus
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "-".to_string());
                    // The Chinese patch stores text as Shift-JIS code points and
                    // paints the real glyphs into the font. Those code points are
                    // not the characters on screen, so the log keeps the voice
                    // id instead of printing them.
                    let cn = s.script_dir.file_name().and_then(|n| n.to_str()) == Some("script_cn");
                    // Mark which art tree the picture came from, so the log
                    // says whether this frame is 4x or 8x art.
                    let bg_tag = match bg_src_scale {
                        Some(scale) if !bg_label.eq_ignore_ascii_case("BLACK") => {
                            format!("{}({}x)", bg_label, scale)
                        }
                        _ => bg_label.clone(),
                    };
                    if cn {
                        eprintln!(
                            "scene {} | line {} | bg {} | sprites {} | focus {} | window {} | voice {}",
                            current_id,
                            line_no,
                            bg_tag,
                            cast,
                            focus_id,
                            if window { "open" } else { "hidden" },
                            voice_id
                        );
                    } else {
                        let who = speaker.as_deref().unwrap_or("-");
                        let preview = body.replace('\n', " / ");
                        let preview: String = preview.chars().take(42).collect();
                        eprintln!(
                            "scene {} | line {} | bg {} | sprites {} | focus {} | window {} | voice {} | {} | {}",
                            current_id,
                            line_no,
                            bg_tag,
                            cast,
                            focus_id,
                            if window { "open" } else { "hidden" },
                            voice_id,
                            who,
                            preview
                        );
                    }
                    // Speed 0, skip, and the headless auto run reveal the line at
                    // once. Otherwise the wait loop types it onto `line_base`.
                    let instant = cmd_skip
                        || auto_text
                        || settings.text_speed == 0
                        || !window
                        || body.is_empty();
                    if instant {
                        line_base = None;
                        line_done = true;
                        reveal = body.chars().count();
                        cur_surf = draw_scene(&bg, &body, speaker.as_deref(), font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                        win.present(&cur_surf);
                    } else {
                        line_base = Some(draw_scene(&bg, "", speaker.as_deref(), font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut()));
                        reveal = 0;
                        line_done = false;
                        type_tick = 0;
                        // The name is on screen before the voice starts. Drawing
                        // the line takes long enough that starting the clip
                        // first played the next sentence over the previous one.
                        if let Some(base) = line_base.as_ref() {
                            let mut frame = clone_surf(base);
                            let active = cmdbar::Active {
                                auto: cmd_auto,
                                skip: cmd_skip,
                                rewind: cmd_rewind,
                                hide: user_hide,
                            };
                            let (fw, fh) = (frame.width as i32, frame.height as i32);
                            bar.draw(&mut frame, None, active, cmd_idle(&settings), fw, fh);
                            cur_surf = frame;
                            win.present(&cur_surf);
                        }
                    }
                    // The picture for this line is up. Start its clip now, so
                    // the `voice:` line in the log belongs to the scene line
                    // just printed and not to the one still on screen.
                    if !cmd_skip {
                        if let (Some(mx), Some(vi), Some(cue)) = (mixer.as_ref(), voices.as_ref(), cue) {
                            skipped_voice = None;
                            play_voice(mx, vi, &settings, &mut voice_stream, cue);
                        } else if settings.page_voice == 1 {
                            skipped_voice = None;
                            if let Some(mx) = mixer.as_ref() {
                                stop_voice(mx, &mut voice_stream);
                            }
                        }
                    }
                    // Blank line after this line's log block (scene line and its
                    // voice line) so consecutive clicks are easy to tell apart.
                    eprintln!();
                    // Remember the current line for save (last_text) and backlog.
                    let logged = match &speaker {
                        Some(sp) => format!("{}：{}", sp, body),
                        None => body.clone(),
                    };
                    backlog.push(logged);
                    if backlog.len() > 200 { backlog.remove(0); }
                }
                vm::lscr::Event::Sprite { char_id, series, variant, slot, frames } => {
                    let old = clone_surf(&cur_surf);
                    let mut changed = false;
                    if let Some(bank) = bank.as_mut() {
                        if let Some(name) = bank.resolve(char_id, series, variant) {
                            if let Some((img, art_scale, src_scale)) = bank.get(&name) {
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
                                    eprintln!(
                                        "  sprite: {} ({}x source, char {}, slot {}, {}f)",
                                        name, src_scale, char_id, idx, frames
                                    );
                                    *dst = Some(ActiveSprite { char_id, name, img, art_scale, src_scale });
                                    changed = true;
                                } else {
                                    eprintln!("  sprite: slot {} out of range", idx);
                                }
                            }
                        } else {
                            eprintln!("  sprite: char {} series {:03} variant {:03} not found", char_id, series, variant);
                        }
                    }
                    if changed {
                        line_base = None;
                        line_done = true;
                        // The previous line has already been advanced past. Leave
                        // the box empty until the next line, so the old speaker's
                        // words are not left sitting on the new face.
                        cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                        if present_fade(win, &old, &cur_surf, fade_len(&settings, cmd_skip, frames)) {
                            return Ok(None);
                        }
                    }
                }
                vm::lscr::Event::SpriteExit { char_id, frames } => {
                    let old = clone_surf(&cur_surf);
                    let before = slots.iter().flatten().filter(|a| a.char_id == char_id).count();
                    for s in slots.iter_mut() {
                        if s.as_ref().is_some_and(|a| a.char_id == char_id) {
                            *s = None;
                        }
                    }
                    eprintln!("  sprite exit: char {} ({}f, removed {})", char_id, frames, before);
                    if before > 0 {
                        line_base = None;
                        line_done = true;
                        cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                        if present_fade(win, &old, &cur_surf, fade_len(&settings, cmd_skip, frames)) {
                            return Ok(None);
                        }
                    }
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
                    cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                    win.present(&cur_surf);
                }
                vm::lscr::Event::BgScene { id, variant, frames } => {
                    // ACT146/147. id 0 and -2 are a scripted black screen.
                    // A missing file keeps the picture already on screen.
                    let old = clone_surf(&cur_surf);
                    if id == 0 || id == -2 {
                        plates.clear();
                        bg_cg = false;
                        bg_id = None;
                        bg_label = "BLACK".to_string();
                        bg = solid_bg(W, H, [0, 0, 0]);
                        bg_scale = None;
                        bg_src_scale = None;
                        // id 0 is a fade-out: the shot is over, so the people
                        // from it must leave. id -2 keeps whoever is standing
                        // (the cut to black in 1010 still has them in frame).
                        // Without this, 依緒 and 雪菜 stay on screen through the
                        // whole rest of 1012_030, including the park and the
                        // narration that is only about かずさ.
                        if id == 0 {
                            slots = Default::default();
                        }
                        eprintln!(
                            "scene {} | bg BLACK (script id {} fade {}f)",
                            current_id, id, frames
                        );
                    } else if let Some(name) = s.bg_index.resolve(id, variant) {
                        if let Some(asset) = load_background_asset(&s.bg_dirs(), &name) {
                            plates.clear();
                            bg_cg = name.starts_with('v') || name.starts_with('V');
                            bg_id = Some((id, variant));
                            bg_label = name.clone();
                            bg_scale = Some(asset.art_scale);
                    bg_src_scale = Some(asset.src_scale);
                            bg = asset.img;
                            eprintln!(
                                "scene {} | bg {} id={} var={} fade={}f",
                                current_id, name, id, variant, frames
                            );
                        } else {
                            eprintln!(
                                "scene {} | bg id={} var={} file {} FAILED to decode, screen stays {}",
                                current_id, id, variant, name, bg_label
                            );
                        }
                    } else {
                        eprintln!(
                            "scene {} | bg MISSING id={} var={}, screen stays {}",
                            current_id, id, variant, bg_label
                        );
                    }
                    line_base = None;
                    line_done = true;
                    cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                    if present_fade(win, &old, &cur_surf, fade_len(&settings, cmd_skip, frames)) {
                        return Ok(None);
                    }
                }
                vm::lscr::Event::EventCg { id, variant, frames } => {
                    // ACT148 replaces the standing shot with one picture.
                    // ACT201 turning a layer on comes through as the same
                    // event (frames 0) and swaps that layer in.
                    let old = clone_surf(&cur_surf);
                    let name = event_cg_name(id, variant);
                    if let Some(asset) = load_background_asset(&s.bg_dirs(), &name) {
                        plates.clear();
                        slots = Default::default();
                        bg_cg = true;
                        bg_id = Some((id, variant));
                        bg_label = name.clone();
                        bg_scale = Some(asset.art_scale);
                    bg_src_scale = Some(asset.src_scale);
                        bg = asset.img;
                        eprintln!(
                            "scene {} | cg {} id={} var={} fade={}f",
                            current_id, name, id, variant, frames
                        );
                    } else {
                        eprintln!(
                            "scene {} | cg MISSING {} id={} var={}, screen stays {}",
                            current_id, name, id, variant, bg_label
                        );
                    }
                    line_base = None;
                    line_done = true;
                    cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                    if present_fade(win, &old, &cur_surf, fade_len(&settings, cmd_skip, frames)) {
                        return Ok(None);
                    }
                }
                vm::lscr::Event::Flash { frames, r, g, b } => {
                    // ACT152. The picture snaps to the colour and eases back.
                    // Skip and "effects off" drop it; a click cuts it short.
                    eprintln!(
                        "scene {} | flash {},{},{} {}f",
                        current_id, r, g, b, frames
                    );
                    let play = !cmd_skip && settings.effect != 0 && frames > 0;
                    if play && present_flash(win, &cur_surf, frames, r, g, b) {
                        return Ok(None);
                    }
                }
                vm::lscr::Event::Overlay { name, slot } => {
                    if let Some(asset) = load_background_asset(&s.bg_dirs(), &name) {
                        let img = &asset.img;
                        plates.retain(|p| p.slot != slot);
                        eprintln!("  overlay: slot {} {} ({}x{})", slot, name, img.width, img.height);
                        plates.push(Plate { slot, name, img: asset.img, art_scale: asset.art_scale });
                        line_base = None;
                        line_done = true;
                        cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                        win.present(&cur_surf);
                    } else {
                        eprintln!("  overlay: slot {} {} NOT FOUND", slot, name);
                    }
                }
                vm::lscr::Event::Window { mode } => {
                    // 0 closes the box for a scene change, 1 opens it again.
                    // Skip is reading the lines, so a scene-change close must
                    // not fold the box or light the hide button.
                    if mode == 0 || mode == 1 {
                        if cmd_skip {
                            eprintln!(
                                "scene {} | window mode {} ignored while skipping",
                                current_id, mode
                            );
                        } else {
                            script_open = mode == 1;
                            window = dialog_open(user_hide, script_open, false);
                            eprintln!(
                                "scene {} | window {}",
                                current_id,
                                if script_open { "open" } else { "closed by script" }
                            );
                            line_base = None;
                            line_done = true;
                            cur_surf = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), window, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                            win.present(&cur_surf);
                        }
                    }
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
                                        window = dialog_open(user_hide, script_open, true);
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
                    // The script's own lines already carry 「１．」「２．」. A made-up
                    // header would push a third option out of the three-line window.
                    let opts: Vec<String> = options.iter().map(|o| clean_text(o)).collect();
                    let body = opts.join("\n");
                    // The choice often arrives while ACT170 has the window closed.
                    // Drawing with that flag drops the text entirely, so the
                    // picture freezes with nothing to pick. The options open it.
                    let line_h = if msg_plate.is_some() {
                        render::DIALOG_LINE
                    } else {
                        font.cell() as i32 + 4
                    };
                    let text_y = if msg_plate.is_some() {
                        render::DIALOG_TEXT_Y
                    } else {
                        let box_h = (H / 4).min(160).max(120);
                        (H - box_h - 44) as i32 + 18
                    };
                    let opt_top = |i: usize| text_y + i as i32 * line_h;
                    // Plate only. The highlight is painted under the glyphs, and
                    // the glyphs are redrawn each frame — a full scene rebuild
                    // is far too slow to repeat while the mouse moves.
                    let base = draw_scene(&bg, "", None, font, &slots, focus, win_size.0 as u32, win_size.1 as u32, tint.as_deref(), true, msg_plate.as_ref(), msg_frame.as_ref(), &plates, scene_look(&settings, bg_cg), vfont.as_mut());
                    let mut hover: Option<usize> = None;
                    let mut bar_hover: Option<cmdbar::Cmd> = None;
                    cur_surf = render::Surface {
                        width: base.width,
                        height: base.height,
                        pixels: base.pixels.clone(),
                    };
                    let (fw, fh) = (cur_surf.width, cur_surf.height);
                    draw_body(&mut cur_surf, font, vfont.as_mut(), &body, None, msg_plate.is_some(), fw, fh, usize::MAX);
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
                            if cmd_rewind || rewind_left > 0 {
                                let announce = !cmd_rewind && rewind_left <= 1;
                                let cruising = cmd_rewind || rewind_left > 1;
                                match take_rewind(&mut history, false) {
                                    None => {
                                        cmd_rewind = false;
                                        rewind_left = 0;
                                    }
                                    Some(dest) => {
                                        if rewind_left > 0 {
                                            rewind_left -= 1;
                                        }
                                        let step = rewind_step(
                                            &dest,
                                            &mut it,
                                            &mut current_id,
                                            &mut rewind_vm,
                                            s,
                                            font,
                                            vfont.as_mut(),
                                            win,
                                            &mut bg,
                                            &mut bg_cg,
                                            &mut bg_label,
                                            &mut bg_id,
                                            &mut tint,
                                            &mut tint_name,
                                            &mut slots,
                                            &mut bank,
                                            &mut focus,
                                            &mut plates,
                                            &mut last_text,
                                            &mut last_speaker,
                                            &mut script_open,
                                            user_hide,
                                            &mut window,
                                            &settings,
                                            msg_plate.as_ref(),
                                            msg_frame.as_ref(),
                                            &mut cur_surf,
                                            &mut line_base,
                                            &mut line_done,
                                            &mut reveal,
                                            mixer.as_ref(),
                                            &mut bgm_track,
                                            &mut fading_bgm,
                                            &mut voice_stream,
                                            voices.as_ref(),
                                            &mut line_voice,
                                            announce,
                                        );
                                        match step {
                                            RewindStep::Shown => {
                                                present_rewind_frame(
                                                    win,
                                                    &cur_surf,
                                                    &bar,
                                                    bar_hover,
                                                    cmdbar::Active {
                                                        auto: cmd_auto,
                                                        skip: false,
                                                        rewind: cmd_rewind,
                                                        hide: user_hide,
                                                    },
                                                    cmd_idle(&settings),
                                                );
                                                if cruising {
                                                    match rewind_cruise_stop(win, &bar, &mut bar_hover) {
                                                        Cruise::Quit => return Ok(None),
                                                        Cruise::Keep => {
                                                            win.delay(16);
                                                            continue;
                                                        }
                                                        Cruise::Stop => {}
                                                    }
                                                }
                                                cmd_rewind = false;
                                                rewind_left = 0;
                                                park_wait = true;
                                                continue 'events;
                                            }
                                            RewindStep::Reload => {
                                                present_rewind_frame(
                                                    win,
                                                    &cur_surf,
                                                    &bar,
                                                    bar_hover,
                                                    cmdbar::Active {
                                                        auto: cmd_auto,
                                                        skip: false,
                                                        rewind: cmd_rewind,
                                                        hide: user_hide,
                                                    },
                                                    cmd_idle(&settings),
                                                );
                                                park_wait = true;
                                                break 'events;
                                            }
                                            RewindStep::Stuck => {}
                                        }
                                    }
                                }
                            }
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
                                frame.fill_rect(hx, y, hw, (line_h + 4) as u32, [40, 70, 120, 255]);
                            }
                            let (fw, fh) = (frame.width, frame.height);
                            draw_body(&mut frame, font, vfont.as_mut(), &body, None, msg_plate.is_some(), fw, fh, usize::MAX);
                            let active = cmdbar::Active {
                                auto: cmd_auto,
                                skip: cmd_skip,
                                rewind: cmd_rewind,
                                hide: user_hide,
                            };
                            let (fw, fh) = (frame.width as i32, frame.height as i32);
                            bar.draw(&mut frame, bar_hover, active, cmd_idle(&settings), fw, fh);
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
                                                cmdbar::Cmd::Auto => {
                                                    cmd_auto = !cmd_auto;
                                                    if cmd_auto {
                                                        cmd_rewind = false;
                                                        rewind_left = 0;
                                                    }
                                                }
                                                cmdbar::Cmd::Rewind => {
                                                    cmd_rewind = !cmd_rewind;
                                                    if cmd_rewind {
                                                        cmd_skip = false;
                                                        cmd_auto = false;
                                                    } else {
                                                        rewind_left = 0;
                                                    }
                                                }
                                                cmdbar::Cmd::Skip => {
                                                    cmd_skip = !cmd_skip;
                                                    if cmd_skip {
                                                        cmd_rewind = false;
                                                        rewind_left = 0;
                                                    }
                                                    window = dialog_open(user_hide, script_open, cmd_skip);
                                                    if let Some(mx) = mixer.as_ref() {
                                                        if cmd_skip {
                                                            stop_voice(mx, &mut voice_stream);
                                                        } else if let (Some(cue), Some(vi)) = (skipped_voice.take(), voices.as_ref()) {
                                                            play_voice(mx, vi, &settings, &mut voice_stream, cue);
                                                        }
                                                    }
                                                },
                                                cmdbar::Cmd::Hide => {
                                                    user_hide = !user_hide;
                                                    window = dialog_open(user_hide, script_open, cmd_skip);
                                                    line_base = None;
                                                    line_done = true;
                                                    cur_surf = draw_scene(
                                                        &bg,
                                                        &last_text,
                                                        last_speaker.as_deref(),
                                                        font,
                                                        &slots,
                                                        focus,
                                                        win_size.0 as u32,
                                                        win_size.1 as u32,
                                                        tint.as_deref(),
                                                        window,
                                                        msg_plate.as_ref(),
                                                        msg_frame.as_ref(),
                                                        &plates,
                                                        scene_look(&settings, bg_cg),
                                                        vfont.as_mut(),
                                                    );
                                                }
                                                other => match handle_cmd(
                                                    other,
                                                    win,
                                                    font,
                                                    vfont.as_mut(),
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
                                sdl2::Input::Wheel { y } if y > 0 => {
                                    cmd_skip = false;
                                    cmd_auto = false;
                                    rewind_left += y.max(1);
                                }
                                sdl2::Input::Up => {
                                    cmd_skip = false;
                                    cmd_auto = false;
                                    rewind_left += 1;
                                }
                                sdl2::Input::Rewind => {
                                    cmd_rewind = !cmd_rewind;
                                    if cmd_rewind {
                                        cmd_skip = false;
                                        cmd_auto = false;
                                    } else {
                                        rewind_left = 0;
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
                    remember(
                        &mut history,
                        capture_line(
                            &current_id,
                            it.snapshot(),
                            bg_id,
                            bg_cg,
                            &bg_label,
                            &tint_name,
                            &slots,
                            focus,
                            &last_text,
                            &last_speaker,
                            bgm_track.map(|(id, _)| id),
                            script_open,
                            &plates,
                            line_voice,
                        ),
                    );
                    let mut cmd_hover: Option<cmdbar::Cmd> = None;
                    let mut dwell: u32 = 0;
                    'wait: loop {
                            if cmd_rewind || rewind_left > 0 {
                                let announce = !cmd_rewind && rewind_left <= 1;
                                let cruising = cmd_rewind || rewind_left > 1;
                                match take_rewind(&mut history, true) {
                                    None => {
                                        cmd_rewind = false;
                                        rewind_left = 0;
                                    }
                                    Some(dest) => {
                                        if rewind_left > 0 {
                                            rewind_left -= 1;
                                        }
                                        backlog.pop();
                                        let step = rewind_step(
                                            &dest,
                                            &mut it,
                                            &mut current_id,
                                            &mut rewind_vm,
                                            s,
                                            font,
                                            vfont.as_mut(),
                                            win,
                                            &mut bg,
                                            &mut bg_cg,
                                            &mut bg_label,
                                            &mut bg_id,
                                            &mut tint,
                                            &mut tint_name,
                                            &mut slots,
                                            &mut bank,
                                            &mut focus,
                                            &mut plates,
                                            &mut last_text,
                                            &mut last_speaker,
                                            &mut script_open,
                                            user_hide,
                                            &mut window,
                                            &settings,
                                            msg_plate.as_ref(),
                                            msg_frame.as_ref(),
                                            &mut cur_surf,
                                            &mut line_base,
                                            &mut line_done,
                                            &mut reveal,
                                            mixer.as_ref(),
                                            &mut bgm_track,
                                            &mut fading_bgm,
                                            &mut voice_stream,
                                            voices.as_ref(),
                                            &mut line_voice,
                                            announce,
                                        );
                                        match step {
                                            RewindStep::Stuck => {}
                                            RewindStep::Shown => {
                                                present_rewind_frame(
                                                    win,
                                                    &cur_surf,
                                                    &bar,
                                                    cmd_hover,
                                                    cmdbar::Active {
                                                        auto: false,
                                                        skip: false,
                                                        rewind: cruising,
                                                        hide: user_hide,
                                                    },
                                                    cmd_idle(&settings),
                                                );
                                                if cruising {
                                                    match rewind_cruise_stop(win, &bar, &mut cmd_hover) {
                                                        Cruise::Quit => return Ok(None),
                                                        Cruise::Keep => {
                                                            win.delay(16);
                                                            continue 'wait;
                                                        }
                                                        Cruise::Stop => {}
                                                    }
                                                }
                                                cmd_rewind = false;
                                                rewind_left = 0;
                                                continue 'wait;
                                            }
                                            RewindStep::Reload => {
                                                present_rewind_frame(
                                                    win,
                                                    &cur_surf,
                                                    &bar,
                                                    cmd_hover,
                                                    cmdbar::Active {
                                                        auto: false,
                                                        skip: false,
                                                        rewind: cmd_rewind,
                                                        hide: user_hide,
                                                    },
                                                    cmd_idle(&settings),
                                                );
                                                park_wait = true;
                                                break 'events;
                                            }
                                        }
                                    }
                                }
                            }
                            if !line_done {
                                if cmd_skip {
                                    reveal = last_text.chars().count();
                                } else {
                                    type_tick += 1;
                                    let (step, every) = match settings.text_speed {
                                        1 => (2usize, 1u32),
                                        2 => (1, 2),
                                        _ => (1, 3),
                                    };
                                    if type_tick >= every {
                                        type_tick = 0;
                                        reveal = reveal.saturating_add(step);
                                    }
                                }
                                if reveal >= last_text.chars().count() {
                                    reveal = last_text.chars().count();
                                    line_done = true;
                                    dwell = 0;
                                }
                            }
                            if line_done {
                                if let Some(base) = line_base.take() {
                                    let mut full = clone_surf(&base);
                                    let (fw, fh) = (full.width, full.height);
                                    draw_body(
                                        &mut full,
                                        font,
                                        vfont.as_mut(),
                                        &last_text,
                                        None,
                                        msg_plate.is_some(),
                                        fw,
                                        fh,
                                        usize::MAX,
                                    );
                                    cur_surf = full;
                                }
                            }
                            let mut frame = if !line_done {
                                if let Some(base) = line_base.as_ref() {
                                    let mut partial = clone_surf(base);
                                    let (fw, fh) = (partial.width, partial.height);
                                    draw_body(
                                        &mut partial,
                                        font,
                                        vfont.as_mut(),
                                        &last_text,
                                        None,
                                        msg_plate.is_some(),
                                        fw,
                                        fh,
                                        reveal,
                                    );
                                    // Saves and the hide button read `cur_surf`, so it
                                    // has to be the text actually on screen.
                                    cur_surf = clone_surf(&partial);
                                    partial
                                } else {
                                    clone_surf(&cur_surf)
                                }
                            } else {
                                clone_surf(&cur_surf)
                            };
                            let active = cmdbar::Active {
                                auto: cmd_auto,
                                skip: cmd_skip,
                                rewind: cmd_rewind,
                                hide: user_hide,
                            };
                            let (fw, fh) = (frame.width as i32, frame.height as i32);
                            bar.draw(&mut frame, cmd_hover, active, cmd_idle(&settings), fw, fh);
                            win.present(&frame);
                            match win.poll_input() {
                                sdl2::Input::Quit => return Ok(None),
                                // F11: toggle 画面模式 without opening the option
                                // screen. The decode scale follows on the next
                                // loop iteration.
                                sdl2::Input::Fullscreen => {
                                    let on = settings.win_disp != 1;
                                    settings.win_disp = if on { 1 } else { 0 };
                                    win.set_fullscreen(on);
                                    set_render_art_scale(win);
                                    if let Err(e) = settings.save() {
                                        eprintln!("settings: save failed: {e}");
                                    }
                                    eprintln!(
                                        "display: {} (decode {}x)",
                                        if on { "fullscreen" } else { "windowed" },
                                        render_art_scale()
                                    );
                                }
                                sdl2::Input::RightClick => {
                                    match settings.r_click {
                                        1 => {
                                            let dirs = s.bg_dirs();
                                            let mut st = menu::Settings::load();
                                            menu::run(win, &dirs, &mut st, mixer.as_ref());
                                            settings = menu::Settings::load();
                                            menu::apply(&settings, mixer.as_ref());
                                            // 「画面模式」 must take effect without a
                                            // trip back to the title screen.
                                            win.set_fullscreen(settings.win_disp == 1);
                                            set_render_art_scale(win);
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
                                            cmdbar::Cmd::Auto => {
                                                cmd_auto = !cmd_auto;
                                                if cmd_auto {
                                                    cmd_rewind = false;
                                                    rewind_left = 0;
                                                }
                                            }
                                            cmdbar::Cmd::Rewind => {
                                                cmd_rewind = !cmd_rewind;
                                                if cmd_rewind {
                                                    cmd_skip = false;
                                                    cmd_auto = false;
                                                    window = dialog_open(user_hide, script_open, false);
                                                    continue 'wait;
                                                }
                                                rewind_left = 0;
                                            }
                                            cmdbar::Cmd::Skip => {
                                                cmd_skip = !cmd_skip;
                                                if cmd_skip {
                                                    cmd_rewind = false;
                                                    rewind_left = 0;
                                                }
                                                window = dialog_open(user_hide, script_open, cmd_skip);
                                                if let Some(mx) = mixer.as_ref() {
                                                    if cmd_skip {
                                                        stop_voice(mx, &mut voice_stream);
                                                    } else if let (Some(cue), Some(vi)) = (skipped_voice.take(), voices.as_ref()) {
                                                        play_voice(mx, vi, &settings, &mut voice_stream, cue);
                                                    }
                                                }
                                            },
                                            cmdbar::Cmd::Hide => {
                                                user_hide = !user_hide;
                                                window = dialog_open(user_hide, script_open, cmd_skip);
                                                line_base = None;
                                                line_done = true;
                                                cur_surf = draw_scene(
                                                    &bg,
                                                    &last_text,
                                                    last_speaker.as_deref(),
                                                    font,
                                                    &slots,
                                                    focus,
                                                    win_size.0 as u32,
                                                    win_size.1 as u32,
                                                    tint.as_deref(),
                                                    window,
                                                    msg_plate.as_ref(),
                                                    msg_frame.as_ref(),
                                                    &plates,
                                                    scene_look(&settings, bg_cg),
                                                    vfont.as_mut(),
                                                );
                                            }
                                            other => match handle_cmd(
                                                other,
                                                win,
                                                font,
                                                vfont.as_mut(),
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
                                                if cmd_rewind || rewind_left > 0 {
                                                    cmd_rewind = false;
                                                    rewind_left = 0;
                                                } else if !line_done {
                                                    reveal = last_text.chars().count();
                                                    line_done = true;
                                                    dwell = 0;
                                                } else {
                                                    break 'wait;
                                                }
                                            }
                                        }
                                    }
                                },
                                sdl2::Input::Advance | sdl2::Input::Right => {
                                    if cmd_rewind || rewind_left > 0 {
                                        cmd_rewind = false;
                                        rewind_left = 0;
                                    } else if !line_done {
                                        reveal = last_text.chars().count();
                                        line_done = true;
                                        dwell = 0;
                                    } else {
                                        break 'wait;
                                    }
                                }
                                // Wheel down is the next line. Wheel up rewinds.
                                sdl2::Input::Wheel { y } if y < 0 => {
                                    if cmd_rewind || rewind_left > 0 {
                                        cmd_rewind = false;
                                        rewind_left = 0;
                                    } else {
                                        if !line_done {
                                            reveal = last_text.chars().count();
                                            line_done = true;
                                        }
                                        break 'wait;
                                    }
                                }
                                sdl2::Input::Wheel { y } if y > 0 => {
                                    cmd_skip = false;
                                    cmd_auto = false;
                                    rewind_left += y.max(1);
                                    continue 'wait;
                                }
                                sdl2::Input::Up => {
                                    cmd_skip = false;
                                    cmd_auto = false;
                                    rewind_left += 1;
                                    continue 'wait;
                                }
                                sdl2::Input::Rewind => {
                                    cmd_rewind = !cmd_rewind;
                                    if cmd_rewind {
                                        cmd_skip = false;
                                        cmd_auto = false;
                                        continue 'wait;
                                    }
                                    rewind_left = 0;
                                }
                                // No new event: keep the hover caption where it is.
                                sdl2::Input::None => {}
                                _ => cmd_hover = None,
                            }
                            if line_done && (auto_text || cmd_auto || cmd_skip) {
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
                        note_seen(&mut seen, &current_id, &last_text);
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
                    line_no = 0;
                    // New scene: standing sprites do not carry over, but keep
                    // the decoded sprite bank.  Clearing it here forces every
                    // character in the next scene through WebP/TGA decoding
                    // again and made ordinary scene jumps look like a load.
                    slots = Default::default();
                    plates.clear();
                    line_base = None;
                    line_done = true;
                    focus = None;
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

/// One line the player already stood on. Rewind restores this instead of
/// replaying the script backwards.
#[derive(Clone)]
struct LineSnap {
    scene: String,
    vm: vm::lscr::InterpState,
    bg: Option<(i64, u32)>,
    bg_cg: bool,
    bg_label: String,
    tint_name: Option<String>,
    slots: Vec<Option<(u32, String)>>,
    focus: Option<u32>,
    text: String,
    speaker: Option<String>,
    bgm: Option<i32>,
    script_open: bool,
    plates: Vec<(u32, String)>,
    voice: Option<vm::lscr::VoiceCue>,
}

const HISTORY_MAX: usize = 400;

fn slot_keys(slots: &[Option<ActiveSprite>; SPRITE_SLOTS]) -> Vec<Option<(u32, String)>> {
    slots.iter().map(|s| s.as_ref().map(|a| (a.char_id, a.name.clone()))).collect()
}

fn plate_keys(plates: &[Plate]) -> Vec<(u32, String)> {
    plates.iter().map(|p| (p.slot, p.name.clone())).collect()
}

fn capture_line(
    scene: &str,
    vm: vm::lscr::InterpState,
    bg: Option<(i64, u32)>,
    bg_cg: bool,
    bg_label: &str,
    tint_name: &Option<String>,
    slots: &[Option<ActiveSprite>; SPRITE_SLOTS],
    focus: Option<u32>,
    text: &str,
    speaker: &Option<String>,
    bgm: Option<i32>,
    script_open: bool,
    plates: &[Plate],
    voice: Option<vm::lscr::VoiceCue>,
) -> LineSnap {
    LineSnap {
        scene: scene.to_string(),
        vm,
        bg,
        bg_cg,
        bg_label: bg_label.to_string(),
        tint_name: tint_name.clone(),
        slots: slot_keys(slots),
        focus,
        text: text.to_string(),
        speaker: speaker.clone(),
        bgm,
        script_open,
        plates: plate_keys(plates),
        voice,
    }
}

fn remember(history: &mut Vec<LineSnap>, snap: LineSnap) {
    let same = history.last().is_some_and(|h| {
        h.scene == snap.scene && h.vm.pc == snap.vm.pc && h.text == snap.text
    });
    if same {
        return;
    }
    history.push(snap);
    if history.len() > HISTORY_MAX {
        history.remove(0);
    }
}

/// `pop_current` drops the line on screen (a text wait). A choice has not
/// pushed its own snapshot, so rewind restores the line already at the top.
fn take_rewind<T: Clone>(history: &mut Vec<T>, pop_current: bool) -> Option<T> {
    if pop_current {
        if history.len() < 2 {
            return None;
        }
        history.pop();
    } else if history.is_empty() {
        return None;
    }
    history.last().cloned()
}

enum RewindStep {
    /// Nothing earlier than the line on screen.
    Stuck,
    /// Picture and interpreter are back on an earlier line of this script.
    Shown,
    /// The earlier line belongs to another script. `rewind_vm` is set.
    Reload,
}

/// Put `dest` on screen and arm the interpreter when the script matches.
fn rewind_step(
    dest: &LineSnap,
    it: &mut vm::lscr::Interp,
    current_id: &mut String,
    rewind_vm: &mut Option<vm::lscr::InterpState>,
    s: &Session,
    font: &render::Font,
    vfont: Option<&mut render::text::VectorFont>,
    win: &mut sdl2::Sdl,
    bg: &mut codec::tga::TgaImage,
    bg_cg: &mut bool,
    bg_label: &mut String,
    bg_id: &mut Option<(i64, u32)>,
    tint: &mut Option<Vec<u8>>,
    tint_name: &mut Option<String>,
    slots: &mut [Option<ActiveSprite>; SPRITE_SLOTS],
    bank: &mut Option<SpriteBank>,
    focus: &mut Option<u32>,
    plates: &mut Vec<Plate>,
    last_text: &mut String,
    last_speaker: &mut Option<String>,
    script_open: &mut bool,
    user_hide: bool,
    window: &mut bool,
    settings: &menu::Settings,
    msg_plate: Option<&codec::tga::TgaImage>,
    msg_frame: Option<&codec::tga::TgaImage>,
    cur_surf: &mut render::Surface,
    line_base: &mut Option<render::Surface>,
    line_done: &mut bool,
    reveal: &mut usize,
    mixer: Option<&audio::Mixer>,
    bgm_track: &mut Option<(i32, usize)>,
    fading_bgm: &mut Option<usize>,
    voice_stream: &mut Option<usize>,
    voices: Option<&VoiceIndex>,
    line_voice: &mut Option<vm::lscr::VoiceCue>,
    announce: bool,
) -> RewindStep {
    if mixer.is_some() {
        stop_voice(mixer.unwrap(), voice_stream);
    }
    if bg_id != &dest.bg || bg_label != &dest.bg_label {
        if let Some((id, variant)) = dest.bg {
            let name = if dest.bg_cg {
                event_cg_name(id, variant)
            } else {
                s.bg_index.resolve(id, variant).unwrap_or_else(|| dest.bg_label.clone())
            };
            if let Some(img) = load_background(&s.bg_dirs(), &name) {
                *bg = img;
            }
        } else {
            *bg = solid_bg(1280, 720, [0, 0, 0]);
        }
        *bg_id = dest.bg;
        *bg_cg = dest.bg_cg;
        *bg_label = dest.bg_label.clone();
    }
    if tint_name != &dest.tint_name {
        *tint_name = dest.tint_name.clone();
        *tint = dest.tint_name.as_deref().and_then(|n| load_tint(&s.bg_dirs(), n));
    }
    if slot_keys(slots) != dest.slots {
        *slots = Default::default();
        for (i, entry) in dest.slots.iter().enumerate() {
            let Some((char_id, name)) = entry else { continue };
            if let Some(bank) = bank.as_mut() {
                let Some((img, art_scale, src_scale)) = bank.get(name) else {
                    continue;
                };
                if let Some(dst) = slots.get_mut(i) {
                    *dst = Some(ActiveSprite {
                        char_id: *char_id,
                        name: name.clone(),
                        img,
                        art_scale,
                        src_scale,
                    });
                }
            }
        }
    }
    if plate_keys(plates) != dest.plates {
        plates.clear();
        for (slot, name) in &dest.plates {
            if let Some(asset) = load_background_asset(&s.bg_dirs(), name) {
                plates.push(Plate { slot: *slot, name: name.clone(), img: asset.img, art_scale: asset.art_scale });
            }
        }
    }
    *focus = dest.focus;
    *last_text = dest.text.clone();
    *last_speaker = dest.speaker.clone();
    *script_open = dest.script_open;
    // Keep the message box up, the way skip does, so the function buttons
    // stay on the window instead of vanishing into the picture.
    *window = dialog_open(user_hide, *script_open, true);
    *line_voice = dest.voice;
    *line_base = None;
    *line_done = true;
    *reveal = last_text.chars().count();
    if let Some(mx) = mixer {
        if bgm_track.map(|(id, _)| id) != dest.bgm {
            if let Some(slot) = fading_bgm.take() {
                mx.stop(slot);
            }
            if let Some((_, slot)) = bgm_track.take() {
                mx.stop(slot);
            }
            if let Some(id) = dest.bgm {
                let dir = s.script_dir.parent().unwrap_or(std::path::Path::new(".")).join("bgm");
                let stem = format!("BGM_{:03}", id);
                let path = [format!("{}.OGG", stem), format!("{}_A.OGG", stem)]
                    .into_iter()
                    .map(|n| dir.join(n))
                    .find(|p| p.is_file());
                if let Some(path) = path {
                    let slot = mx.play_cat(&path.to_string_lossy(), 1.0, true, audio::mixer::CAT_BGM);
                    if slot != usize::MAX {
                        *bgm_track = Some((id, slot));
                    }
                }
            }
        }
    }
    let win_size = win.window_size();
    *cur_surf = draw_scene(
        bg,
        last_text,
        last_speaker.as_deref(),
        font,
        slots,
        *focus,
        win_size.0 as u32,
        win_size.1 as u32,
        tint.as_deref(),
        *window,
        msg_plate,
        msg_frame,
        plates,
        scene_look(settings, *bg_cg),
        vfont,
    );
    if announce {
        if let (Some(mx), Some(vi), Some(cue)) = (mixer, voices, dest.voice) {
            play_voice(mx, vi, settings, voice_stream, cue);
        }
    }
    eprintln!("rewind: scene {} pc {}", dest.scene, dest.vm.pc);
    if dest.scene == *current_id {
        it.restore(dest.vm.clone());
        RewindStep::Shown
    } else {
        *current_id = dest.scene.clone();
        *rewind_vm = Some(dest.vm.clone());
        RewindStep::Reload
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

fn centered_plate(w: i32, h: i32) -> (i32, i32) {
    ((1280 - w) / 2, (720 - h) / 2)
}

fn in_box(sx: i32, sy: i32, b: (i32, i32, i32, i32)) -> bool {
    sx >= b.0 && sx < b.0 + b.2 && sy >= b.1 && sy < b.1 + b.3
}

/// Yes on the left, no on the right, along the bottom of the confirm plate.
fn ask_buttons(px: i32, py: i32, pw: i32, ph: i32) -> [(i32, i32, i32, i32); 2] {
    let bw = 160;
    let bh = 40;
    let x = px + (pw - bw * 2) / 2;
    let y = py + ph - bh - 28;
    [(x, y, bw, bh), (x + bw, y, bw, bh)]
}

fn blit_line(surf: &mut render::Surface, text: Option<&(u32, u32, Vec<u8>)>, px: i32, py: i32, pw: i32, ph: i32) {
    if let Some((tw, th, rgba)) = text {
        let x = px + (pw - *tw as i32) / 2;
        let y = py + (ph - *th as i32) / 2 - 8;
        surf.blit_rgba(rgba, *tw, *th, x.max(px + 8), y.max(py + 8));
    }
}

fn paint_toast(
    frame: &render::Surface,
    plate: Option<&codec::tga::TgaImage>,
    text: Option<&(u32, u32, Vec<u8>)>,
) -> render::Surface {
    let mut s = clone_surf(frame);
    s.dim(110);
    let (pw, ph) = plate
        .map(|p| (p.width as i32, p.height as i32))
        .unwrap_or((480, 112));
    let (px, py) = centered_plate(pw, ph);
    if let Some(p) = plate {
        s.blit(p, px, py);
    } else {
        s.fill_rect(px, py, pw as u32, ph as u32, [16, 90, 104, 255]);
    }
    blit_line(&mut s, text, px, py, pw, ph);
    s
}

fn paint_confirm(
    frame: &render::Surface,
    plate: Option<&codec::tga::TgaImage>,
    buttons: Option<&codec::tga::TgaImage>,
    text: Option<&(u32, u32, Vec<u8>)>,
    hover: Option<bool>,
) -> render::Surface {
    let mut s = clone_surf(frame);
    s.dim(140);
    let (pw, ph) = plate
        .map(|p| (p.width as i32, p.height as i32))
        .unwrap_or((544, 216));
    let (px, py) = centered_plate(pw, ph);
    if let Some(p) = plate {
        s.blit(p, px, py);
    } else {
        s.fill_rect(px, py, pw as u32, ph as u32, [16, 90, 104, 255]);
    }
    // The prompt sits in the upper half; the 是/否 row owns the bottom.
    if let Some((tw, th, rgba)) = text {
        let x = px + (pw - *tw as i32) / 2;
        s.blit_rgba(rgba, *tw, *th, x.max(px + 16), py + 48);
    }
    if let Some(img) = buttons {
        let boxes = ask_buttons(px, py, pw, ph);
        for (i, rect) in boxes.iter().enumerate() {
            let hot = hover == Some(i == 0);
            let row = if hot { 1 } else { 0 };
            s.blit_region(img, (i as i32) * 160, row * 40, 160, 40, rect.0, rect.1);
        }
    }
    s
}

/// Short confirmation that a quick save landed. A click dismisses it early.
/// `false` means the player closed the window.
fn show_toast(
    win: &mut sdl2::Sdl,
    frame: &render::Surface,
    dirs: &[&std::path::Path],
    message: &str,
) -> bool {
    let plate = load_background(dirs, "sys_00110.tga");
    let text = render::text::with_ui_font(22, |f| f.line_rgba(message, 22)).flatten();
    let surf = paint_toast(frame, plate.as_ref(), text.as_ref());
    win.present(&surf);
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_millis(900) {
        match win.poll_input() {
            sdl2::Input::Quit => return false,
            sdl2::Input::Pointer { pressed: true, .. }
            | sdl2::Input::Advance
            | sdl2::Input::Right
            | sdl2::Input::Back => return true,
            _ => win.delay(16),
        }
    }
    true
}

/// Yes / no over `sys_00100` + `sys_00101`. `None` is the window being closed.
fn ask_yes_no(
    win: &mut sdl2::Sdl,
    frame: &render::Surface,
    dirs: &[&std::path::Path],
    message: &str,
) -> Option<bool> {
    let plate = load_background(dirs, "sys_00100.tga");
    let buttons = load_background(dirs, "sys_00101.tga");
    let text = render::text::with_ui_font(22, |f| f.line_rgba(message, 22)).flatten();
    let (pw, ph) = plate
        .as_ref()
        .map(|p| (p.width as i32, p.height as i32))
        .unwrap_or((544, 216));
    let (px, py) = centered_plate(pw, ph);
    let boxes = ask_buttons(px, py, pw, ph);
    let mut mx = -1i32;
    let mut my = -1i32;
    loop {
        let hover = if in_box(mx, my, boxes[0]) {
            Some(true)
        } else if in_box(mx, my, boxes[1]) {
            Some(false)
        } else {
            None
        };
        let surf = paint_confirm(frame, plate.as_ref(), buttons.as_ref(), text.as_ref(), hover);
        win.present(&surf);
        match win.poll_input() {
            sdl2::Input::Quit => return None,
            sdl2::Input::Back | sdl2::Input::RightClick => return Some(false),
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, 1280, 720);
                mx = sx;
                my = sy;
                if pressed {
                    if in_box(sx, sy, boxes[0]) {
                        return Some(true);
                    }
                    if in_box(sx, sy, boxes[1]) {
                        return Some(false);
                    }
                }
            }
            _ => {}
        }
        win.delay(16);
    }
}

/// Handle a command-bar button that opens a modal screen (保存/读档/设置/履历).
/// The toggle buttons (自动/跳过/隐藏) are handled inline by the caller.
fn handle_cmd(
    cmd: cmdbar::Cmd,
    win: &mut sdl2::Sdl,
    font: &render::Font,
    // The history screen is the last modal that still drew from the atlas;
    // the vector font rides along so it matches the dialog it replays.
    vfont: Option<&mut render::text::VectorFont>,
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
            let cn = s.script_dir.file_name().and_then(|n| n.to_str()) == Some("script_cn");
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
            let ok = st.save_to(&save::quick_path()).is_ok();
            let message = match (cn, ok) {
                (true, true) => "已快速保存",
                (true, false) => "快速保存失败",
                (false, true) => "クイックセーブしました",
                (false, false) => "クイックセーブに失敗しました",
            };
            eprintln!("scene {} | quick save {}", current_id, if ok { "ok" } else { "FAILED" });
            if show_toast(win, frame, &s.bg_dirs(), message) {
                CmdOutcome::Stay
            } else {
                CmdOutcome::Quit
            }
        }
        cmdbar::Cmd::QuickLoad => {
            let cn = s.script_dir.file_name().and_then(|n| n.to_str()) == Some("script_cn");
            let Some(st) = save::SaveState::load_from(&save::quick_path()) else {
                let message = if cn { "没有快速存档" } else { "クイックセーブがありません" };
                eprintln!("scene {} | quick load: no file", current_id);
                return if show_toast(win, frame, &s.bg_dirs(), message) {
                    CmdOutcome::Stay
                } else {
                    CmdOutcome::Quit
                };
            };
            let message = if cn { "要读取快速存档吗？" } else { "クイックロードしますか？" };
            match ask_yes_no(win, frame, &s.bg_dirs(), message) {
                Some(true) => {
                    eprintln!("scene {} | quick load confirmed", current_id);
                    CmdOutcome::Loaded(st)
                }
                Some(false) => CmdOutcome::Stay,
                None => CmdOutcome::Quit,
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
            win.set_fullscreen(settings.win_disp == 1);
            set_render_art_scale(win);
            CmdOutcome::Stay
        }
        cmdbar::Cmd::Backlog => {
            show_backlog(win, font, vfont, backlog, frame, &s.bg_dirs());
            CmdOutcome::Stay
        }
        // toggles are handled by the caller
        _ => CmdOutcome::Stay,
    }
}

/// One visual row of the text history. An empty text is the gap between two
/// logged lines; a name row is drawn with the speaker style so the history
/// reads like the dialog it came from.
struct BacklogRow {
    text: String,
    name: bool,
}

/// Wrap the history into rows. With a vector font the wrap follows the font's
/// own advances instead of the atlas cell pitch, so a Chinese line stops using
/// the Japanese layout's 28px-per-character rhythm.
fn backlog_rows(lines: &[String], vf: Option<&render::text::VectorFont>) -> Vec<BacklogRow> {
    let max_w = 1080;
    let mut rows = Vec::new();
    for line in lines {
        let (name, body) = match line.split_once('：') {
            Some((n, b)) if !n.is_empty() => (Some(n), b),
            _ => (None, line.as_str()),
        };
        if let Some(n) = name {
            push_wrapped(&mut rows, n, max_w, vf, true);
        }
        if body.is_empty() {
            rows.push(BacklogRow { text: String::new(), name: false });
        } else {
            for part in body.split('\n') {
                push_wrapped(&mut rows, part, max_w, vf, false);
            }
        }
        rows.push(BacklogRow { text: String::new(), name: false });
    }
    rows
}

fn push_wrapped(
    rows: &mut Vec<BacklogRow>,
    text: &str,
    max_w: i32,
    vf: Option<&render::text::VectorFont>,
    name: bool,
) {
    let mut buf = String::new();
    let mut x = 0i32;
    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        let adv = match vf {
            Some(f) => f.measure_char(ch),
            None if (ch as u32) < 128 => 14,
            None => 28,
        };
        if x + adv > max_w && !buf.is_empty() {
            rows.push(BacklogRow { text: std::mem::take(&mut buf), name });
            x = 0;
        }
        buf.push(ch);
        x += adv;
    }
    if !buf.is_empty() {
        rows.push(BacklogRow { text: buf, name });
    }
}

fn paint_backlog(
    scene: &render::Surface,
    font: &render::Font,
    // Chinese mode draws the history with the vector font; Japanese keeps the
    // bitmap atlas, whose cell pitch the row layout below is built around.
    mut vf: Option<&mut render::text::VectorFont>,
    rows: &[BacklogRow],
    top: usize,
    per_page: usize,
    header: Option<&codec::tga::TgaImage>,
    scroll: Option<&codec::tga::TgaImage>,
    hover_bar: bool,
    dragging: bool,
) -> render::Surface {
    const LINE_H: i32 = 32;
    const TEXT_X: i32 = 72;
    const TEXT_Y: i32 = 52;
    const TEXT_BOTTOM: i32 = 688;
    const BAR_X: i32 = 1232;
    const BAR_Y: i32 = 40;
    const BAR_H: i32 = 640;
    const COL_W: i32 = 32;
    let mut surf = render::Surface {
        width: scene.width,
        height: scene.height,
        pixels: scene.pixels.clone(),
    };
    surf.dim(120);
    if let Some(h) = header {
        surf.blit(h, 0, 0);
    }
    let mut y = TEXT_Y;
    // The history screen is authored at 1280x720 like the rest of its panels,
    // so the vector text stays at the layout size rather than scaling with the
    // window.
    if let Some(f) = vf.as_mut() {
        let _ = f.set_size(render::DIALOG_FONT_720);
    }
    let body_style = render::text::TextStyle::for_role(render::text::TextRole::Body);
    let name_style = render::text::TextStyle::for_role(render::text::TextRole::Name);
    for row in rows.iter().skip(top).take(per_page) {
        if !row.text.is_empty() {
            match vf.as_mut() {
                Some(f) => {
                    let f = &mut **f;
                    let style = if row.name { name_style } else { body_style };
                    let baseline = f.baseline_in_box(y, LINE_H);
                    f.draw_styled(&mut surf, &row.text, TEXT_X, baseline, &style);
                }
                None => font.draw_pitch(&mut surf, &row.text, TEXT_X, y, render::DIALOG_PITCH),
            }
        }
        y += LINE_H;
        if y >= TEXT_BOTTOM {
            break;
        }
    }
    let total = rows.len();
    let max_top = total.saturating_sub(per_page);
    if let Some(sb) = scroll {
        let col = if dragging {
            2
        } else if hover_bar {
            1
        } else {
            0
        };
        surf.blit_region(sb, col * COL_W, 0, COL_W as u32, BAR_H as u32, BAR_X, BAR_Y);
        if max_top > 0 {
            let track_y = BAR_Y + COL_W;
            let track_h = BAR_H - COL_W * 2;
            let thumb_h = ((track_h as usize) * per_page / total.max(1)).max(28) as i32;
            let thumb_h = thumb_h.min(track_h);
            let thumb_y = track_y + (track_h - thumb_h) * top as i32 / max_top as i32;
            let src_h = thumb_h.min(BAR_H - 80).max(8) as u32;
            surf.blit_region(sb, 2 * COL_W, 80, COL_W as u32, src_h, BAR_X, thumb_y);
        }
    }
    surf
}

/// 文本历史. The banner is the game's own `sys_00032` ("Backlog"), the bar on
/// the right is `sys_00030`, and the lines sit on the dimmed scene in the
/// same face as the message window.
fn show_backlog(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    mut vf: Option<&mut render::text::VectorFont>,
    lines: &[String],
    scene: &render::Surface,
    dirs: &[&std::path::Path],
) {
    const W: u32 = 1280;
    const LINE_H: i32 = 32;
    const TEXT_X: i32 = 72;
    const TEXT_Y: i32 = 52;
    const TEXT_BOTTOM: i32 = 688;
    const BAR_X: i32 = 1232;
    const BAR_Y: i32 = 40;
    const BAR_H: i32 = 640;
    const COL_W: i32 = 32;
    let rows = backlog_rows(lines, vf.as_deref());
    let per_page = ((TEXT_BOTTOM - TEXT_Y) / LINE_H) as usize;
    let total = rows.len();
    let max_top = total.saturating_sub(per_page);
    let mut top = max_top;
    let header = load_background(dirs, "sys_00032.tga");
    let scroll = load_background(dirs, "sys_00030.tga");
    let mut pointer: Option<(i32, i32)> = None;
    let mut drag_top: Option<i32> = None;
    loop {
        let hover_bar = pointer
            .map(|(x, yy)| x >= BAR_X && x < BAR_X + COL_W && yy >= BAR_Y && yy < BAR_Y + BAR_H)
            .unwrap_or(false);
        let surf = paint_backlog(
            scene,
            font,
            vf.as_deref_mut(),
            &rows,
            top,
            per_page,
            header.as_ref(),
            scroll.as_ref(),
            hover_bar,
            drag_top.is_some(),
        );
        win.present(&surf);
        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::Back | sdl2::Input::RightClick | sdl2::Input::Advance | sdl2::Input::Right => {
                return;
            }
            sdl2::Input::Up => top = top.saturating_sub(1),
            sdl2::Input::Down => top = (top + 1).min(max_top),
            sdl2::Input::Wheel { y } => {
                let step = (y.abs() as usize).max(1) * 3;
                if y > 0 {
                    top = top.saturating_sub(step);
                } else {
                    top = (top + step).min(max_top);
                }
            }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W, 720);
                pointer = Some((sx, sy));
                let on_bar = sx >= BAR_X && sx < BAR_X + COL_W && sy >= BAR_Y && sy < BAR_Y + BAR_H;
                if pressed && on_bar {
                    let track_y = BAR_Y + COL_W;
                    let track_h = BAR_H - COL_W * 2;
                    if sy < track_y {
                        top = top.saturating_sub(1);
                    } else if sy >= track_y + track_h {
                        top = (top + 1).min(max_top);
                    } else if max_top > 0 {
                        let rel = (sy - track_y).clamp(0, track_h);
                        top = (rel as usize * max_top / track_h as usize).min(max_top);
                        drag_top = Some(sy);
                    }
                } else if pressed {
                    return;
                } else {
                    drag_top = None;
                }
            }
            sdl2::Input::Drag { x, y } => {
                let (sx, sy) = win.surface_pos(x, y, W, 720);
                pointer = Some((sx, sy));
                if drag_top.is_some() && max_top > 0 {
                    let track_y = BAR_Y + COL_W;
                    let track_h = BAR_H - COL_W * 2;
                    let rel = (sy - track_y).clamp(0, track_h);
                    top = (rel as usize * max_top / track_h as usize).min(max_top);
                }
            }
            _ => win.delay(16),
        }
    }
}

/// The Chinese patch keeps its text in `script_cn`: Shift-JIS codes whose
/// glyphs were repainted, which `codec::sjis::decode_cn` resolves.
fn is_cn_script(dir: &std::path::Path) -> bool {
    dir.file_name().and_then(|n| n.to_str()) == Some("script_cn")
}

/// Load and parse an LSCR program (`.bnr` + companion `.txt` token pool).
fn load_program(script_dir: &std::path::Path, id: &str, cn: bool) -> Option<vm::lscr::Program> {
    let bnr = std::fs::read(script_dir.join(format!("{}.bnr", id))).ok()?;
    let txt = std::fs::read(script_dir.join(format!("{}.txt", id))).ok()?;
    match vm::lscr::Program::load(&bnr, &txt, cn) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("lscr parse error for {}: {}", id, e);
            None
        }
    }
}

/// Locate a directory of loose character sprites (legacy layout helper).
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

/// Character sprite bank backed by a directory of loose TGA or WebP files.
///
/// On construction it indexes the directory once, so resolution is an in-memory
/// lookup. (The previous archive-backed version sliced filenames by byte index,
/// which panicked on the archives' one non-ASCII entry name; a directory scan
/// and a prefix rule that walks `char`s cannot panic.)
struct SpriteBank {
    /// Scaled sprite trees, ascending; the decode scale picks the active one.
    trees: Vec<(u32, std::path::PathBuf)>,
    dir: std::path::PathBuf,
    /// Scale of the pixels in `dir`; 0 before the first retarget.
    scale: u32,
    /// lowercased file names present in the directory
    index: std::collections::HashSet<String>,
    /// file name -> (decoded pixels, scale of those pixels, scale of the
    /// source tree they were read from)
    cache: std::collections::HashMap<String, (std::sync::Arc<codec::tga::TgaImage>, u32, u32)>,
    /// High-resolution sprites are large after WebP decode. Do not retain the
    /// entire 1,600-file library in RAM as the story advances.
    cache_limit: usize,
}

impl SpriteBank {
    fn new(trees: Vec<(u32, std::path::PathBuf)>) -> SpriteBank {
        let mut bank = SpriteBank {
            trees,
            dir: std::path::PathBuf::new(),
            scale: 0,
            index: Default::default(),
            cache: Default::default(),
            cache_limit: 64,
        };
        bank.retarget(render_art_scale());
        bank
    }

    /// Point the bank at the tree matching `want`, rebuilding the name index
    /// and dropping sprites decoded at another scale.
    fn retarget(&mut self, want: u32) {
        let mut order: Vec<usize> = (0..self.trees.len()).collect();
        order.sort_by_key(|&i| {
            let scale = self.trees[i].0;
            if scale == want {
                0
            } else if scale > want {
                1 + (scale - want)
            } else {
                100 + (want - scale)
            }
        });
        let Some(&i) = order.first() else { return };
        let (scale, dir) = self.trees[i].clone();
        if self.scale == scale && self.dir == dir {
            return;
        }
        self.scale = scale;
        self.dir = dir;
        self.index.clear();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let name = match e.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".tga") || lower.ends_with(".webp") {
                    self.index.insert(lower);
                }
            }
        }
        self.cache.clear();
        eprintln!("sprites: retargeted to {} ({}x)", self.dir.display(), scale);
        // Decoded sprites are 4x larger per art-scale step; keep the cache
        // small enough that the working set stays in the tens of MB.
        self.cache_limit = match scale {
            8 => 8,
            4 => 16,
            2 => 24,
            _ => 64,
        };
    }

    /// Returns the decoded pixels and the art scale they represent. A cached
    /// image keeps the scale it was decoded with, so resizing the window never
    /// mixes pixel scales between already-loaded and newly loaded sprites.
    fn get(&mut self, name: &str) -> Option<(std::sync::Arc<codec::tga::TgaImage>, u32, u32)> {
        if self.scale != render_art_scale() {
            self.retarget(render_art_scale());
        }
        let key = name.to_ascii_lowercase();
        if !self.cache.contains_key(&key) {
            let path = self.dir.join(&key);
            // Shared with the background loader, so a sprite the prefetch
            // thread already decoded costs nothing here.
            let img = (*artcache::load(&path)?).clone();
            let (src_scale, art_scale) = codec::image::art_scales(&path);
            let art_scale = art_scale.max(1);
            if self.cache.len() >= self.cache_limit {
                if let Some(old) = self.cache.keys().next().cloned() {
                    self.cache.remove(&old);
                }
            }
            self.cache.insert(key.clone(), (std::sync::Arc::new(img), art_scale, src_scale.max(1)));
        }
        self.cache
            .get(&key)
            .map(|(img, scale, src)| (std::sync::Arc::clone(img), *scale, *src))
    }

    /// Resolve `(char, series, variant)` to a real sprite file name.
    ///
    /// The shipped scripts' `(series, variant)` values match the file names
    /// directly for the vast majority of cues, so the search walks *outward*
    /// from the exact value (0, -1, +1, -2, +2). The previous order started at
    /// -2, which made almost every pose collapse onto `<prefix>001101.tga`.
    ///
    /// Only this character's own prefix is searched. Filling a missing pose
    /// with somebody else's file put the wrong person on stage.
    ///
    /// The neutral-pose fallback (`...101`) is only tried after the whole
    /// neighbourhood search has failed.
    fn resolve(&self, cid: u32, series: u32, variant: u32) -> Option<String> {
        let prefixes: Vec<String> = sprite_prefixes(cid).iter().map(|s| s.to_string()).collect();
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
                    for ext in [".webp", ".tga"] {
                        let name = format!("{}{:03}{:03}{}", pf, s as u32, v as u32, ext).to_ascii_lowercase();
                        if self.index.contains(&name) {
                            return Some(name);
                        }
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
                    for ext in [".webp", ".tga"] {
                        let name = format!("{}{:03}{:03}{}", pf, s as u32, v, ext).to_ascii_lowercase();
                        if self.index.contains(&name) {
                            return Some(name);
                        }
                    }
                }
            }
        }
        None
    }

}

/// A character currently on screen.
struct LoadedAsset {
    img: codec::tga::TgaImage,
    /// Effective scale of the decoded pixels relative to 1280x720 layout
    /// coordinates. UI assets have already been normalized and use 1.
    art_scale: u32,
    /// Scale of the source tree the file was read from (4 for `bg_4x`, 8 for
    /// `bg_8x`, 1 for the original TGA tree). Printed next to the asset name so
    /// the log says where the pixels came from.
    src_scale: u32,
}

struct ActiveSprite {
    char_id: u32,
    #[allow(dead_code)]
    name: String,
    img: std::sync::Arc<codec::tga::TgaImage>,
    art_scale: u32,
    /// Scale of the sprite tree the file came from (4x or 8x), for the log.
    src_scale: u32,
}

/// An ACT176 plate (item, effect, or a layer of a composite CG).
struct Plate {
    slot: u32,
    name: String,
    img: codec::tga::TgaImage,
    art_scale: u32,
}

/// How the message window and the scene behind it are drawn this frame.
struct Look {
    alpha: u8,
    shade: u8,
}

fn scene_look(settings: &menu::Settings, cg: bool) -> Look {
    let levels = menu::LEVELS as u32;
    let level = if cg { settings.alpha_cg } else { settings.alpha_novel };
    let level = level.clamp(0, menu::LEVELS) as u32;
    Look {
        alpha: (level * 255 / levels) as u8,
        // Novel text gets a mild dim so the window reads. CG stays untouched,
        // and the top of the slider (10) only reaches about a third darker.
        shade: if cg {
            0
        } else {
            (settings.alpha_back.clamp(0, menu::LEVELS) as u32 * 90 / levels) as u8
        },
    }
}

fn cmd_idle(settings: &menu::Settings) -> u8 {
    (settings.alpha_command.clamp(0, menu::LEVELS) as u32 * 100 / menu::LEVELS as u32) as u8
}

/// The box is up when the player has not hidden it, and either the script
/// has it open or skip is running. Skip keeps the lines on screen; the hide
/// button only follows the player's own toggle.
fn dialog_open(user_hide: bool, script_open: bool, skipping: bool) -> bool {
    !user_hide && (script_open || skipping)
}

fn clone_surf(s: &render::Surface) -> render::Surface {
    render::Surface {
        width: s.width,
        height: s.height,
        pixels: s.pixels.clone(),
    }
}

fn surface_from_tga(img: &codec::tga::TgaImage) -> render::Surface {
    render::Surface {
        width: img.width,
        height: img.height,
        pixels: img.rgba.clone(),
    }
}

/// What the player did while fast-rewind was stepping.
enum Cruise {
    Keep,
    Stop,
    Quit,
}

fn present_rewind_frame(
    win: &mut sdl2::Sdl,
    cur: &render::Surface,
    bar: &cmdbar::CmdBar,
    hover: Option<cmdbar::Cmd>,
    active: cmdbar::Active,
    idle: u8,
) {
    let mut frame = clone_surf(cur);
    let (fw, fh) = (frame.width as i32, frame.height as i32);
    bar.draw(&mut frame, hover, active, idle, fw, fh);
    win.present(&frame);
}

/// Click, Enter, Page Up, or the rewind button stops the cruise. Moving the
/// pointer only updates the hover, so the buttons stay usable. The stop is
/// taken on release: the press and the release are one click, and leaving
/// the release for the line-wait would advance the script.
fn rewind_cruise_stop(win: &mut sdl2::Sdl, bar: &cmdbar::CmdBar, hover: &mut Option<cmdbar::Cmd>) -> Cruise {
    match win.poll_input() {
        sdl2::Input::Quit => Cruise::Quit,
        sdl2::Input::Advance | sdl2::Input::Right | sdl2::Input::Rewind | sdl2::Input::RightClick => Cruise::Stop,
        sdl2::Input::Wheel { y } if y < 0 => Cruise::Stop,
        sdl2::Input::Release { x, y } => {
            let (sx, sy) = win.surface_pos(x, y, 1280, 720);
            *hover = bar.hit(sx, sy);
            Cruise::Stop
        }
        sdl2::Input::Pointer { x, y, .. } => {
            let (sx, sy) = win.surface_pos(x, y, 1280, 720);
            *hover = bar.hit(sx, sy);
            Cruise::Keep
        }
        sdl2::Input::Drag { x, y } => {
            let (sx, sy) = win.surface_pos(x, y, 1280, 720);
            *hover = bar.hit(sx, sy);
            Cruise::Keep
        }
        _ => Cruise::Keep,
    }
}

fn fade_len(settings: &menu::Settings, skipping: bool, frames: u32) -> u32 {
    if skipping || settings.effect == 0 {
        0
    } else {
        frames.min(240)
    }
}

/// Crossfade `from` into `to`. A click or a key lands on the destination.
/// Returns true when the player closed the window.
/// Snap the screen to `rgb` and fade back to `base` over `frames` at 60 fps.
/// A click or a key lands on the picture. Returns true when the window closed.
fn present_flash(win: &mut sdl2::Sdl, base: &render::Surface, frames: u32, r: u8, g: u8, b: u8) -> bool {
    let mut buf = vec![0u8; base.pixels.len()];
    for remain in (0..=frames).rev() {
        render::apply_flash(&mut buf, &base.pixels, r, g, b, remain, frames);
        win.present_raw(&buf, base.width, base.height);
        if remain == 0 {
            break;
        }
        match win.poll_input() {
            sdl2::Input::Quit => return true,
            sdl2::Input::None | sdl2::Input::Pointer { pressed: false, .. } | sdl2::Input::Drag { .. } => {}
            _ => {
                win.present(base);
                return false;
            }
        }
        win.delay(16);
    }
    false
}

fn present_fade(win: &mut sdl2::Sdl, from: &render::Surface, to: &render::Surface, frames: u32) -> bool {
    if frames == 0 || from.pixels.len() != to.pixels.len() || from.width != to.width {
        win.present(to);
        return false;
    }
    let mut buf = vec![0u8; to.pixels.len()];
    for step in 1..=frames {
        render::lerp_pixels(&mut buf, &from.pixels, &to.pixels, step, frames);
        win.present_raw(&buf, to.width, to.height);
        match win.poll_input() {
            sdl2::Input::Quit => return true,
            sdl2::Input::None
            | sdl2::Input::Pointer { pressed: false, .. }
            | sdl2::Input::Drag { .. } => {}
            _ => {
                win.present(to);
                return false;
            }
        }
        win.delay(16);
    }
    win.present(to);
    false
}

fn load_seen() -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    if let Ok(text) = std::fs::read_to_string(save::dir().join("seen.txt")) {
        for line in text.lines() {
            if !line.is_empty() {
                set.insert(line.to_string());
            }
        }
    }
    set
}

fn note_seen(set: &mut std::collections::HashSet<String>, scene: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let key = format!("{}\t{}", scene, text.replace('\n', " "));
    if !set.insert(key.clone()) {
        return;
    }
    let dir = save::dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("seen.txt"))
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", key);
    }
}

fn place_overlays<'a>(plates: &'a [Plate], w: u32, h: u32) -> Vec<(&'a codec::tga::TgaImage, i32, i32, u32)> {
    plates
        .iter()
        .map(|p| {
            let scale = p.art_scale.max(1);
            let logical_w = (p.img.width / scale).max(1);
            let logical_h = (p.img.height / scale).max(1);
            let (x, y) = if logical_w >= 1100 || logical_h >= 600 {
                (0, 0)
            } else {
                (
                    (w as i32 - logical_w as i32) / 2,
                    (h as i32 - logical_h as i32) / 2,
                )
            };
            (&p.img, x, y, scale)
        })
        .collect()
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

/// The composited picture without the dialogue, kept between lines.
///
/// Advancing the text used to stretch the background, blend the sprites, apply
/// the colour filter and stamp the message window again for every line. Only a
/// stage change (background, sprite, overlay, filter, window size) really needs
/// that work; printing a line only has to repaint the text on a copy.
struct SceneLayer {
    key: u64,
    w: u32,
    h: u32,
    surf: render::Surface,
}

fn scene_layer_store() -> &'static std::sync::Mutex<Option<SceneLayer>> {
    static LAYER: std::sync::OnceLock<std::sync::Mutex<Option<SceneLayer>>> = std::sync::OnceLock::new();
    LAYER.get_or_init(|| std::sync::Mutex::new(None))
}

/// FNV-1a over everything the picture is made of. Image identity is the pixel
/// buffer address: the same decoded picture keeps its address for as long as
/// the scene holds it, and a new load necessarily has a different one.
fn hash_u64(state: &mut u64, value: u64) {
    *state = (*state ^ value).wrapping_mul(0x100_0000_01b3);
}

fn scene_art_key(
    bg: &codec::tga::TgaImage,
    draws: &[render::SpriteDraw],
    tint: Option<&[u8]>,
    window: bool,
    plate: Option<&codec::tga::TgaImage>,
    frame: Option<&codec::tga::TgaImage>,
    look: &Look,
    overlays: &[(&codec::tga::TgaImage, i32, i32, u32)],
    w: u32,
    h: u32,
) -> u64 {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    hash_u64(&mut key, bg.rgba.as_ptr() as usize as u64);
    hash_u64(&mut key, bg.width as u64);
    for sp in draws {
        hash_u64(&mut key, sp.img.rgba.as_ptr() as usize as u64);
        hash_u64(&mut key, sp.logical_w as u64);
        hash_u64(&mut key, sp.x as u64);
        hash_u64(&mut key, sp.y as u64);
        hash_u64(&mut key, sp.alpha as u64);
    }
    hash_u64(&mut key, tint.map(|t| t.as_ptr() as usize as u64).unwrap_or(0));
    hash_u64(&mut key, tint.map(|t| t.len() as u64).unwrap_or(0));
    hash_u64(&mut key, window as u64);
    hash_u64(&mut key, plate.map(|p| p.rgba.as_ptr() as usize as u64).unwrap_or(0));
    hash_u64(&mut key, frame.map(|p| p.rgba.as_ptr() as usize as u64).unwrap_or(0));
    hash_u64(&mut key, look.alpha as u64);
    hash_u64(&mut key, look.shade as u64);
    for (img, x, y, scale) in overlays {
        hash_u64(&mut key, img.rgba.as_ptr() as usize as u64);
        hash_u64(&mut key, *x as u64);
        hash_u64(&mut key, *y as u64);
        hash_u64(&mut key, *scale as u64);
    }
    hash_u64(&mut key, w as u64);
    hash_u64(&mut key, h as u64);
    key
}

/// Composite the scene picture, reusing the previous frame when nothing about
/// the stage has changed.
#[allow(clippy::too_many_arguments)]
fn scene_art(
    bg: &codec::tga::TgaImage,
    draws: &[render::SpriteDraw],
    tint: Option<&[u8]>,
    window: bool,
    plate: Option<&codec::tga::TgaImage>,
    frame: Option<&codec::tga::TgaImage>,
    look: &Look,
    overlays: &[(&codec::tga::TgaImage, i32, i32, u32)],
    w: u32,
    h: u32,
    key: u64,
) -> render::Surface {
    let mut store = match scene_layer_store().lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if let Some(layer) = store.as_ref() {
        if layer.key == key && layer.w == w && layer.h == h {
            return clone_surf(&layer.surf);
        }
    }
    let surf = render::compose_scene_art(
        bg, w, h, draws, tint, window, plate, frame, look.alpha, look.shade, overlays,
    );
    *store = Some(SceneLayer { key, w, h, surf: clone_surf(&surf) });
    surf
}


/// Composite background + sprites + dialog box + text.
fn draw_scene(
    bg: &codec::tga::TgaImage,
    text: &str,
    speaker: Option<&str>,
    font: &render::Font,
    slots: &[Option<ActiveSprite>; SPRITE_SLOTS],
    focus: Option<u32>,
    // Size of the frame to return - the window, not the 1280x720 layout.
    out_w: u32,
    out_h: u32,
    tint: Option<&[u8]>,
    // 「ウィンドウ一時消去」: false hides the dialog box and its text.
    window: bool,
    plate: Option<&codec::tga::TgaImage>,
    frame: Option<&codec::tga::TgaImage>,
    plates: &[Plate],
    look: Look,
    // Vector font for the dialogue, or None to keep the bitmap atlas.
    vf: Option<&mut render::text::VectorFont>,
) -> render::Surface {
    let t0 = std::time::Instant::now();
    // The Japanese bitmap atlas still belongs to the 1280x720 compositor. The
    // Chinese path uses vector text and can composite high-res art directly at
    // the window size without stretching the bitmap glyphs.
    let hires = vf.is_some() && (out_w > render::LAYOUT_W || out_h > render::LAYOUT_H);
    // Dim the listeners only when the speaker is actually standing there.
    // 春希 speaks thousands of times and is placed twice in the whole game:
    // he is the camera, and the person on screen is who he is talking to.
    // Dimming everyone in that case made the listener look like the wrong
    // speaker. When the speaker is on stage, the others drop well below
    // full brightness so the lit face is the name in the window.
    let speaker_on_stage = focus.is_some_and(|id| {
        slots.iter().flatten().any(|s| s.char_id == id)
    });
    let draws: Vec<render::SpriteDraw> = SPRITE_DRAW_ORDER
        .iter()
        .filter_map(|&i| {
            let s = slots[i].as_ref()?;
            let alpha = if speaker_on_stage && focus != Some(s.char_id) {
                96
            } else {
                255
            };
            let sprite_scale = s.art_scale.max(1);
            let logical_w = (s.img.width / sprite_scale).max(1);
            let logical_h = (s.img.height / sprite_scale).max(1);
            Some(render::SpriteDraw {
                img: &s.img,
                logical_w,
                logical_h,
                x: SPRITE_ANCHORS[i] - logical_w as i32 / 2,
                y: (render::LAYOUT_H as i32 - logical_h as i32).max(0),
                alpha,
            })
        })
        .collect();
    let overlays = place_overlays(plates, render::LAYOUT_W, render::LAYOUT_H);
    // With a vector font the text is drawn after the frame is scaled, so the
    // atlas path hands the text to the compositor and the vector path does not.
    let render_w = if hires { out_w } else { render::LAYOUT_W };
    let render_h = if hires { out_h } else { render::LAYOUT_H };
    let key = scene_art_key(
        bg, &draws, tint, window, plate, frame, &look, &overlays, render_w, render_h,
    );
    let t_art = std::time::Instant::now();
    let mut s = scene_art(
        bg, &draws, tint, window, plate, frame, &look, &overlays, render_w, render_h, key,
    );
    tlog("scene art", t_art);
    // The bitmap atlas stamps its glyphs onto this layer; the vector path
    // draws later, after the frame has been scaled to the window.
    if !vf.is_some() && window {
        render::draw_dialog_text(&mut s, font, text, plate.is_some(), render_w, render_h, usize::MAX);
    }
    let out = match vf {
        Some(vf) => {
            let mut s = if !hires && (out_w, out_h) != (render::LAYOUT_W, render::LAYOUT_H) {
                s.scaled_to(out_w, out_h)
            } else {
                s
            };
            if window {
                // The name sits on the upper rim of the oval, not inside 【】
                // in the line, and only the game's own window has a rim.
                let name = if plate.is_some() { speaker } else { None };
                let td = std::time::Instant::now();
                render::draw_dialog_vector(
                    &mut s,
                    vf,
                    text,
                    name,
                    out_w as i32,
                    out_h as i32,
                    usize::MAX,
                );
                tlog("dialog text", td);
            }
            s
        }
        None => {
            // The name sits on the upper rim of the oval, not inside 【】 in the line.
            if window && plate.is_some() {
                if let Some(name) = speaker {
                    if !name.is_empty() {
                        font.draw_pitch(&mut s, name, render::NAME_X, render::NAME_Y, render::DIALOG_PITCH);
                    }
                }
            }
            s
        }
    };
    tlog("draw_scene", t0);
    out
}

/// Draw the dialogue body onto a frame that is already composited, at the size
/// the frame actually is.
///
/// With a vector font the frame is window-sized and the glyphs are rasterized
/// at that size; otherwise the atlas is stamped onto the 1280x720 layer. `limit`
/// is how many characters to reveal.
fn draw_body(
    frame: &mut render::Surface,
    font: &render::Font,
    vf: Option<&mut render::text::VectorFont>,
    text: &str,
    speaker: Option<&str>,
    plate: bool,
    out_w: u32,
    out_h: u32,
    limit: usize,
) {
    match vf {
        Some(vf) => {
            let name = if plate { speaker } else { None };
            render::draw_dialog_vector(frame, vf, text, name, out_w as i32, out_h as i32, limit);
        }
        None => render::draw_dialog_text(
            frame,
            font,
            text,
            plate,
            render::LAYOUT_W,
            render::LAYOUT_H,
            limit,
        ),
    }
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

/// CG stills from `bg/v*.tga`. Left and right (or the two halves of the
/// screen) page through them; Esc or the right button returns to the title.
fn cg_names(dirs: &[&std::path::Path]) -> Vec<String> {
    let mut names = Vec::new();
    for d in dirs {
        let rd = match std::fs::read_dir(d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let low = name.to_ascii_lowercase();
            if low.starts_with('v')
                && (low.ends_with(".tga") || low.ends_with(".webp"))
                && !names.iter().any(|n: &String| n.eq_ignore_ascii_case(&name))
            {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

fn browse_cg(win: &mut sdl2::Sdl, font: &render::Font, dirs: &[&std::path::Path], names: &[String]) {
    if names.is_empty() {
        return;
    }
    const W: u32 = 1280;
    const H: u32 = 720;
    let mut i = 0usize;
    loop {
        let img = load_background(dirs, &names[i]).unwrap_or_else(|| solid_bg(W, H, [0, 0, 0]));
        let mut surf = render::render_scene_ex(&img, "", font, W, H, &[], None, false, None, None, 255, 0, &[]);
        let caption = format!("{}   {} / {}", names[i], i + 1, names.len());
        if let Some((rw, rh, rgba)) =
            render::text::with_ui_font(22, |f| f.block_rgba(&[caption], 700, 40, 22)).flatten()
        {
            surf.fill_rect(24, H as i32 - rh as i32 - 28, rw + 16, rh + 8, [0, 0, 0, 160]);
            surf.blit_rgba(&rgba, rw, rh, 32, H as i32 - rh as i32 - 24);
        }
        win.present(&surf);
        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::RightClick | sdl2::Input::Back => return,
            sdl2::Input::Advance | sdl2::Input::Right | sdl2::Input::Down => i = (i + 1) % names.len(),
            sdl2::Input::Up => i = (i + names.len() - 1) % names.len(),
            sdl2::Input::Pointer { x, y, pressed: true } => {
                let (sx, _) = win.surface_pos(x, y, W, H);
                if sx < (W as i32) / 2 {
                    i = (i + names.len() - 1) % names.len();
                } else {
                    i = (i + 1) % names.len();
                }
            }
            _ => win.delay(16),
        }
    }
}

/// 「特别模式」. Labels are the system font. The highlight bar and the logo
/// stay the original art. 「时间线」 replaces the atlas row 场景回放.
enum SpecialPick {
    Replay,
    Cg,
    Music,
    /// An entry the original lists but this build does not open yet.
    Notice,
    Back,
}

fn crop_image(
    image: &codec::tga::TgaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
) -> Option<codec::tga::TgaImage> {
    if x >= image.width || y >= image.height {
        return None;
    }
    let w = w.min(image.width - x);
    let h = h.min(image.height - y);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for row in 0..h as usize {
        let src = ((y as usize + row) * image.width as usize + x as usize) * 4;
        let dst = row * w as usize * 4;
        rgba[dst..dst + w as usize * 4]
            .copy_from_slice(&image.rgba[src..src + w as usize * 4]);
    }
    Some(codec::tga::TgaImage { width: w, height: h, rgba })
}

fn load_ui_asset(
    dirs: &[&std::path::Path],
    name: &str,
    expected_w: u32,
    expected_h: u32,
) -> Option<codec::tga::TgaImage> {
    for dir in dirs {
        let path = dir.join(name);
        if let Ok(img) = codec::image::decode_ui_file(&path) {
            if img.width == expected_w && img.height == expected_h {
                return Some(img);
            }
        }
    }
    None
}

/// Rasterize menu labels once. Each line is the system CJK font, white on
/// transparent, ready to be tinted white or gold per frame.
fn menu_labels(items: &[&str], size: u32) -> Vec<Option<(u32, u32, Vec<u8>)>> {
    items
        .iter()
        .map(|s| render::text::with_ui_font(size, |f| f.line_rgba(s, size)).flatten())
        .collect()
}

/// Title highlight change. `SE_9011` is the short two-tap click.
fn cursor_se_path(bg_dirs: &[&std::path::Path]) -> Option<std::path::PathBuf> {
    let mut roots = Vec::new();
    for d in bg_dirs {
        if let Some(p) = d.parent() {
            roots.push(p.to_path_buf());
        }
    }
    for root in roots {
        for name in ["SE_9011.WAV", "SE_9011.wav", "SE_9011.OGG", "SE_9011.ogg"] {
            let p = root.join("se").join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn play_cursor(mx: Option<&audio::Mixer>, se: Option<&std::path::Path>) {
    let (Some(mx), Some(se)) = (mx, se) else { return };
    mx.play_cat(&se.to_string_lossy(), 1.0, false, audio::mixer::CAT_SE);
}

/// Font size that has the same height, in window pixels, as `layout_px` on
/// the 720-tall layout. Labels are drawn after the scene is scaled, so this
/// is the size they must be rasterized at.
fn layout_font_px(layout_px: u32, window_h: i32) -> u32 {
    ((layout_px as i32 * window_h.max(1)) / 720).clamp(8, 160) as u32
}

/// Where the highlight sits in window pixels. `layout_y` stays fractional so
/// a maximized window can move the bar one screen pixel at a time.
fn bar_window_y(layout_y: f32, wh: i32) -> i32 {
    (layout_y * wh.max(1) as f32 / 720.0).round() as i32
}

/// Slide the highlight toward `target`. At 720px tall this is the original
/// per-frame ease. A taller window caps each step at about six screen pixels,
/// otherwise one layout pixel becomes several screen pixels and the bar hops.
fn slide_bar(bar_y: &mut f32, target: f32, wh: i32) {
    let left = target - *bar_y;
    let snap = if wh > 720 { 720.0 / wh as f32 } else { 0.5 };
    if left.abs() <= snap {
        *bar_y = target;
        return;
    }
    let mut step = left * 0.30;
    if wh > 720 {
        let max_layout = 6.0 * 720.0 / wh as f32;
        if step.abs() > max_layout {
            step = max_layout.copysign(step);
        }
    }
    *bar_y += step;
}

/// Logo and copyright, already stretched to the window, plus the highlight
/// oval at that same pixel size. Rebuilt only when the window size changes,
/// so the bar can animate without resampling the whole frame.
struct FittedMenu {
    ww: i32,
    wh: i32,
    stamp: u64,
    plate: render::Surface,
    bar: [Option<render::Surface>; 2],
}

fn scale_sprite(
    img: &codec::tga::TgaImage,
    sx: i32,
    sy: i32,
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
) -> render::Surface {
    let mut s = render::Surface::new(sw.max(1), sh.max(1), [0, 0, 0, 0]);
    for row in 0..sh as i32 {
        for col in 0..sw as i32 {
            let sxp = sx + col;
            let syp = sy + row;
            if sxp < 0 || syp < 0 || sxp >= img.width as i32 || syp >= img.height as i32 {
                continue;
            }
            let si = ((syp as u32 * img.width + sxp as u32) * 4) as usize;
            let di = ((row as u32 * s.width + col as u32) * 4) as usize;
            s.pixels[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
        }
    }
    if dw == s.width && dh == s.height {
        return s;
    }
    for px in s.pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = (px[0] as u32 * a / 255) as u8;
        px[1] = (px[1] as u32 * a / 255) as u8;
        px[2] = (px[2] as u32 * a / 255) as u8;
    }
    let mut up = s.scaled_to(dw, dh);
    for px in up.pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 0 {
            continue;
        }
        px[0] = (px[0] as u32 * 255 / a).min(255) as u8;
        px[1] = (px[1] as u32 * 255 / a).min(255) as u8;
        px[2] = (px[2] as u32 * 255 / a).min(255) as u8;
    }
    up
}

fn ensure_fitted(
    fitted: &mut Option<FittedMenu>,
    ww: i32,
    wh: i32,
    stamp: u64,
    layout: impl FnOnce() -> render::Surface,
    bar: Option<&codec::tga::TgaImage>,
    bar_w: u32,
    bar_h: u32,
) {
    if let Some(f) = fitted.as_ref() {
        if f.ww == ww && f.wh == wh && f.stamp == stamp {
            return;
        }
    }
    let layout = layout();
    let plate = if ww as u32 == layout.width && wh as u32 == layout.height {
        layout
    } else {
        layout.scaled_to(ww as u32, wh as u32)
    };
    let dw = (bar_w as i32 * ww / 1280).max(1) as u32;
    let dh = (bar_h as i32 * wh / 720).max(1) as u32;
    let mut frames = [None, None];
    if let Some(img) = bar {
        frames[0] = Some(scale_sprite(img, 0, 0, bar_w, bar_h, dw, dh));
        if img.height >= bar_h * 2 {
            frames[1] = Some(scale_sprite(img, 0, bar_h as i32, bar_w, bar_h, dw, dh));
        }
    }
    *fitted = Some(FittedMenu { ww, wh, stamp, plate, bar: frames });
}

fn paint_bar(frame: &mut render::Surface, fitted: &FittedMenu, which: usize, layout_y: f32, x0: i32) {
    let Some(img) = fitted.bar[which].as_ref() else { return };
    let dx = x0 * fitted.ww / 1280;
    let dy = bar_window_y(layout_y, fitted.wh);
    frame.blit_rgba(&img.pixels, img.width, img.height, dx, dy);
}

fn draw_menu_labels(
    surf: &mut render::Surface,
    labels: &[Option<(u32, u32, Vec<u8>)>],
    sel: usize,
    x0: i32,
    y0: i32,
    item_w: i32,
    item_h: i32,
    pitch: i32,
    ww: i32,
    wh: i32,
) {
    let shadow = ((2 * wh) / 720).max(1);
    for (i, label) in labels.iter().enumerate() {
        let Some((w, h, rgba)) = label else { continue };
        let rgb = if i == sel { [254, 232, 136] } else { [255, 255, 255] };
        let ay = y0 + i as i32 * pitch;
        let x0w = x0 * ww / 1280;
        let y0w = ay * wh / 720;
        let x1w = (x0 + item_w) * ww / 1280;
        let y1w = (ay + item_h) * wh / 720;
        blit_tinted_label(
            surf,
            rgba,
            *w,
            *h,
            x0w + (x1w - x0w - *w as i32) / 2,
            y0w + (y1w - y0w - *h as i32) / 2,
            rgb,
            shadow,
        );
    }
}

/// System-font stand-in for one menu cell. The atlas glyphs are white with a
/// navy outline `(35, 45, 66)` about two pixels thick. The outline is drawn
/// first so it surrounds the fill instead of sitting as a drop shadow.
fn blit_tinted_label(
    surf: &mut render::Surface,
    rgba: &[u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rgb: [u8; 3],
    stroke_px: i32,
) {
    let outline = [35, 45, 66];
    let r = stroke_px.max(1);
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let i = ((row as u32 * w + col as u32) * 4) as usize;
            let a = rgba[i + 3];
            if a == 0 {
                continue;
            }
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    surf.blend_pixel(
                        x + col + dx,
                        y + row + dy,
                        [outline[0], outline[1], outline[2], a],
                    );
                }
            }
        }
    }
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let i = ((row as u32 * w + col as u32) * 4) as usize;
            let a = rgba[i + 3];
            if a == 0 {
                continue;
            }
            surf.blend_pixel(x + col, y + row, [rgb[0], rgb[1], rgb[2], a]);
        }
    }
}

fn run_special(
    win: &mut sdl2::Sdl,
    bg_dirs: &[&std::path::Path],
    cn: bool,
    mixer: Option<&audio::Mixer>,
    cursor: Option<&std::path::Path>,
) -> SpecialPick {
    const W: u32 = 1280;
    const H: u32 = 720;
    const ITEM_W: i32 = 320;
    const ITEM_H: i32 = 40;
    const PITCH: i32 = 40;
    // Same horizontal placement as the title items. The six rows and the
    // first item's y come from WA2.exe: the special page lists T0100 rows
    // 9,10,11,12,13,8 and lays them out from y=380 at a pitch of 40.
    const X0: i32 = (W as i32 - ITEM_W) / 2;
    const Y0: i32 = 380;
    const ROWS: [i32; 6] = [9, 10, 11, 12, 13, 8];
    const N: usize = ROWS.len();
    const BACKDROP: [u8; 4] = [1, 29, 89, 255];

    let bar = load_background(bg_dirs, "T0101.tga");
    let logo = load_background(bg_dirs, "T0011.tga");
    let items: [&str; 6] = if cn {
        ["电子小说", "CG模式", "时间线", "音乐模式", "声优访谈", "返回"]
    } else {
        ["デジタルノベル", "CGモード", "タイムライン", "サウンドモード", "ボイス", "戻る"]
    };
    let head_text = if cn { "特别内容" } else { "スペシャル" };
    let mut label_px = 0u32;
    let mut labels = Vec::new();
    let mut head_px = 0u32;
    let mut head: Option<(u32, u32, Vec<u8>)> = None;
    let mut fitted: Option<FittedMenu> = None;

    let pick = |sel: usize| match ROWS[sel] {
        10 => SpecialPick::Cg,
        11 => SpecialPick::Replay,
        12 => SpecialPick::Music,
        8 => SpecialPick::Back,
        _ => SpecialPick::Notice,
    };
    let mut sel = 0usize;
    let mut bar_y = Y0 as f32;
    // WA2_SPECIAL_AUTO=<n> pre-selects item n and confirms it (verification
    // runs: no input synthesis needed to reach the timeline headlessly).
    let mut auto_pick: Option<usize> = std::env::var_os("WA2_SPECIAL_AUTO")
        .and_then(|v| v.to_string_lossy().parse::<usize>().ok())
        .map(|p| p.min(N - 1));
    if let Some(p) = auto_pick {
        sel = p;
        bar_y = (Y0 + p as i32 * PITCH) as f32;
    }
    loop {
        if auto_pick.take().is_some() {
            win.delay(400);
            return pick(sel);
        }
        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::RightClick | sdl2::Input::Back => {
                return SpecialPick::Back;
            }
            sdl2::Input::Up => {
                sel = (sel + N - 1) % N;
                play_cursor(mixer, cursor);
            }
            sdl2::Input::Down => {
                sel = (sel + 1) % N;
                play_cursor(mixer, cursor);
            }
            sdl2::Input::Advance | sdl2::Input::Right | sdl2::Input::Choice(_) => return pick(sel),
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W, H);
                if let Some(i) = (0..N).find(|i| {
                    let iy = Y0 + *i as i32 * PITCH;
                    sx >= X0 && sx < X0 + ITEM_W && sy >= iy && sy < iy + ITEM_H
                }) {
                    if i != sel {
                        sel = i;
                        play_cursor(mixer, cursor);
                    }
                    if pressed {
                        return pick(sel);
                    }
                }
            }
            _ => {}
        }
        let (ww, wh) = win.window_size();
        let ww = ww.max(1);
        let wh = wh.max(1);
        let target = (Y0 + sel as i32 * PITCH) as f32;
        slide_bar(&mut bar_y, target, wh);

        // T0100 draws each label about 29px tall inside the 40px row.
        let ipx = layout_font_px(32, wh);
        if ipx != label_px {
            label_px = ipx;
            labels = menu_labels(&items, ipx);
        }
        let hpx = layout_font_px(16, wh);
        if hpx != head_px {
            head_px = hpx;
            head = render::text::with_ui_font(hpx, |f| f.line_rgba(head_text, hpx)).flatten();
        }

        ensure_fitted(
            &mut fitted,
            ww,
            wh,
            0,
            || {
                let mut s = render::Surface::new(W, H, BACKDROP);
                if let Some(ref lg) = logo {
                    s.blit(lg, (W as i32 - lg.width as i32) / 2, 24);
                }
                s
            },
            bar.as_ref(),
            ITEM_W as u32,
            ITEM_H as u32,
        );
        let fitted = fitted.as_ref().unwrap();
        let mut frame = clone_surf(&fitted.plate);
        paint_bar(&mut frame, fitted, 0, bar_y, X0);
        if let Some((w, h, rgba)) = head.as_ref() {
            let shadow = ((2 * wh) / 720).max(1);
            blit_tinted_label(
                &mut frame,
                rgba,
                *w,
                *h,
                (ww - *w as i32) / 2,
                (Y0 - 44) * wh / 720,
                [255, 255, 255],
                shadow,
            );
        }
        draw_menu_labels(&mut frame, &labels, sel, X0, Y0, ITEM_W, ITEM_H, PITCH, ww, wh);
        win.present(&frame);
        win.delay(16);
    }
}

/// Music mode.  BGM files are indexed by track number; `_A` is preferred
/// when both internal variants are present because it is the normal in-game
/// stream.  The player keeps one looping stream alive and never decodes the
/// whole library up front.
fn run_music(
    win: &mut sdl2::Sdl,
    bg_dirs: &[&std::path::Path],
    bgm_dir: &std::path::Path,
    cn: bool,
    mixer: Option<&audio::Mixer>,
    cursor: Option<&std::path::Path>,
) {
    const W: u32 = 1280;
    const H: u32 = 720;
    let mut choices: std::collections::BTreeMap<i32, (u8, std::path::PathBuf)> =
        std::collections::BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(bgm_dir) {
        for ent in rd.flatten() {
            let path = ent.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let low = stem.to_ascii_lowercase();
            let Some(rest) = low.strip_prefix("bgm_") else { continue };
            let number = rest.split('_').next().and_then(|s| s.parse::<i32>().ok());
            let Some(number) = number else { continue };
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !ext.eq_ignore_ascii_case("ogg") { continue; }
            let rank = if low.ends_with("_a") { 0 } else if low.ends_with("_b") { 2 } else { 1 };
            if choices.get(&number).map(|(old, _)| rank < *old).unwrap_or(true) {
                choices.insert(number, (rank, path));
            }
        }
    }
    let tracks: Vec<(i32, std::path::PathBuf)> = choices.into_iter().map(|(id, (_, p))| (id, p)).collect();
    let header = load_ui_asset(bg_dirs, "sys_04000.tga", 1280, 40);
    let logo = load_ui_asset(bg_dirs, "sys_04010.tga", 304, 168);
    let footer = load_ui_asset(bg_dirs, "sys_04021.tga", 288, 40);
    let info_bar = load_ui_asset(bg_dirs, "sys_04030.tga", 1024, 56);
    let select_sheet = load_ui_asset(bg_dirs, "sys_04100.tga", 400, 128);
    let name_sheet = load_ui_asset(bg_dirs, "sys_04101.tga", 344, 1512);
    let backdrop = load_background(bg_dirs, "T0000.tga")
        .map(|img| surface_from_tga(&img).scaled_to(W, H));
    let mut selected = 0usize;
    let mut playing: Option<(i32, usize)> = None;
    let mut notice = String::new();
    let labels: Vec<Option<codec::tga::TgaImage>> = tracks.iter().enumerate().map(|(i, _)| {
        name_sheet.as_ref().and_then(|sheet| crop_image(sheet, 0, (i as u32 * 24).min(sheet.height.saturating_sub(24)), 344, 24))
    }).collect();
    eprintln!("music: indexed {} tracks from {}", tracks.len(), bgm_dir.display());

    loop {
        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::RightClick | sdl2::Input::Back => break,
            sdl2::Input::Up => {
                if !tracks.is_empty() {
                    selected = (selected + tracks.len() - 1) % tracks.len();
                    play_cursor(mixer, cursor);
                }
            }
            sdl2::Input::Down => {
                if !tracks.is_empty() {
                    selected = (selected + 1) % tracks.len();
                    play_cursor(mixer, cursor);
                }
            }
            sdl2::Input::Advance | sdl2::Input::Right | sdl2::Input::Choice(_) => {
                if let Some((id, path)) = tracks.get(selected) {
                    if let Some(mx) = mixer {
                        if let Some((_, slot)) = playing.take() { mx.stop(slot); }
                        let slot = mx.play_cat(&path.to_string_lossy(), 1.0, true, audio::mixer::CAT_BGM);
                        if slot != usize::MAX {
                            playing = Some((*id, slot));
                            notice = format!("{}  {}", if cn { "正在播放" } else { "Playing" }, id);
                            eprintln!("music: playing {} -> {}", id, path.display());
                        } else {
                            notice = if cn { "无法播放此曲目".into() } else { "Unable to play track".into() };
                        }
                    } else {
                        notice = if cn { "音频设备不可用".into() } else { "Audio unavailable".into() };
                    }
                }
            }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W, H);
                if (94..510).contains(&sy) {
                    let col = ((sx - 42) / 400) as usize;
                    let row = ((sy - 94) / 32) as usize;
                    if col < 3 && row < 13 {
                        let index = (selected / 39) * 39 + col * 13 + row;
                        if index < tracks.len() {
                            if selected != index {
                                selected = index;
                                play_cursor(mixer, cursor);
                            }
                            if pressed {
                                if let Some(mx) = mixer {
                                if let Some((_, slot)) = playing.take() { mx.stop(slot); }
                                let (id, path) = &tracks[selected];
                                let slot = mx.play_cat(&path.to_string_lossy(), 1.0, true, audio::mixer::CAT_BGM);
                                if slot != usize::MAX {
                                    playing = Some((*id, slot));
                                    notice = format!("{}  {}", if cn { "正在播放" } else { "Playing" }, id);
                                    eprintln!("music: playing {} -> {}", id, path.display());
                                }
                            }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let page_size = 39usize;
        let top = (selected / page_size) * page_size;
        let mut frame = backdrop.as_ref().map(clone_surf)
            .unwrap_or_else(|| render::Surface::new(W, H, [9, 31, 43, 255]));
        if let Some(h) = header.as_ref() { frame.blit(h, 0, 0); }
        if let Some(l) = logo.as_ref() { frame.blit(l, 42, 48); }
        for (row, i) in (top..(top + page_size).min(tracks.len())).enumerate() {
            let col = row / 13;
            let line = row % 13;
            let x = 42 + col as i32 * 400;
            let y = 94 + line as i32 * 32;
            if let Some(sheet) = select_sheet.as_ref() {
                let sy = if i == selected { 64 } else { 32 };
                if let Some(mark) = crop_image(sheet, 0, sy, 400, 32) {
                    frame.blit(&mark, x, y);
                }
            }
            if let Some(name) = labels[i].as_ref() {
                frame.blit(name, x + 46, y + 4);
            } else {
                let fallback = format!("BGM_{:03}", tracks[i].0);
                if let Some((w, h, rgba)) = render::text::with_ui_font(22, |f| f.line_rgba(&fallback, 22)).flatten() {
                    frame.blit_rgba(&rgba, w, h, x + 46, y + 4);
                }
            }
        }
        if tracks.is_empty() {
            notice = if cn { "未找到 BGM 文件".into() } else { "No BGM files".into() };
        }
        if !notice.is_empty() {
            if let Some((w, h, rgba)) = render::text::with_ui_font(20, |f| f.line_rgba(&notice, 20)).flatten() {
                frame.blit_rgba(&rgba, w, h, 440, 610);
            }
        }
        if let Some(bar) = info_bar.as_ref() { frame.blit(bar, 128, 532); }
        if let Some(name) = labels.get(selected).and_then(|x| x.as_ref()) {
            frame.blit(name, 165, 560);
        }
        if let Some(f) = footer.as_ref() { frame.blit(f, 496, 620); }
        win.present(&frame);
        win.delay(16);
    }
    if let Some((_, slot)) = playing {
        if let Some(mx) = mixer { mx.stop(slot); }
    }
}

/// What the title screen resolved to.
enum TitleAction {
    Start,
    /// A scene picked on the timeline.
    Jump(String),
    /// Resume from a save slot picked on the load screen.
    Load(save::SaveState),
    Quit,
}

/// One falling flake. Layout numbers are 1280×720 pixels; the draw scales them.
struct TitleFlake {
    x: f32,
    y: f32,
    /// Downward speed, layout pixels per second.
    speed: f32,
    phase: f32,
    /// Which of the eight sizes. 0 is the largest.
    kind: usize,
    alpha: u8,
}

/// Title snowfall. The original loads `weather.tga` and draws the white flake
/// cell over the street. Kind 248..255 is a size of 36..8 px. The exe's own
/// alpha (about 96..40) vanishes on this bright plate, so the flakes are
/// drawn brighter and there are more of them.
struct TitleSnow {
    ww: i32,
    wh: i32,
    flakes: Vec<TitleFlake>,
    sprites: Vec<render::Surface>,
    rng: u32,
}

fn snow_rng(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}

impl TitleSnow {
    const COUNT: usize = 400;
    const KINDS: usize = 8;

    fn new() -> TitleSnow {
        let mut rng = 0x00a2_5e1du32;
        let mut flakes = Vec::with_capacity(Self::COUNT);
        for i in 0..Self::COUNT {
            flakes.push(Self::make(&mut rng, i % Self::KINDS, true));
        }
        TitleSnow { ww: 0, wh: 0, flakes, sprites: Vec::new(), rng }
    }

    /// `kind` 0..8 maps to the exe's flake id 248..255.
    fn make(rng: &mut u32, kind: usize, anywhere: bool) -> TitleFlake {
        let id = 248 + kind;
        let span = 100 * (256 - id);
        let r = snow_rng(rng) % span as u32;
        let v = (r as f32 / 100.0 + 1.0) * 0.5;
        let x_span = (0x240 - id) * 4;
        let y_span = (0x1b4 - id) * 4;
        let inset = (256 - id) * 2;
        let x = (snow_rng(rng) % x_span as u32) as f32 - inset as f32;
        let y = if anywhere {
            (snow_rng(rng) % y_span as u32) as f32 - inset as f32
        } else {
            -((snow_rng(rng) % 48) as f32) - 8.0
        };
        TitleFlake {
            x,
            y,
            // vy * 5/40 per frame at 60fps. Larger ids are the small, slower flakes.
            speed: v * 7.5,
            phase: (snow_rng(rng) % 256) as f32 * std::f32::consts::TAU / 256.0,
            kind,
            // Exe's 96..40, lifted so a white flake still reads on the street.
            alpha: (((256 - id) * 8 + 32) * 2).min(220) as u8,
        }
    }

    fn ensure(&mut self, ww: i32, wh: i32, sheet: &codec::tga::TgaImage) {
        if self.ww == ww && self.wh == wh && self.sprites.len() == Self::KINDS {
            return;
        }
        self.sprites.clear();
        for kind in 0..Self::KINDS {
            let id = 248 + kind;
            let size = ((257 - id) as i32 * wh / 720).max(2) as u32;
            // weather.tga cell (0, 32): the white flake the title's source rect selects.
            self.sprites.push(scale_sprite(sheet, 0, 32, 32, 32, size, size));
        }
        self.ww = ww;
        self.wh = wh;
    }

    fn step(&mut self, dt: f32) {
        for flake in &mut self.flakes {
            flake.y += flake.speed * dt;
            flake.phase += dt * 1.8;
            flake.x += flake.phase.sin() * 18.0 * dt;
            if flake.y > 760.0 || flake.x < -40.0 || flake.x > 1320.0 {
                *flake = Self::make(&mut self.rng, flake.kind, false);
            }
        }
    }

    fn paint(&self, frame: &mut render::Surface, wh: i32) {
        for flake in &self.flakes {
            let Some(sp) = self.sprites.get(flake.kind) else { continue };
            let x = (flake.x * wh as f32 / 720.0).round() as i32 - sp.width as i32 / 2;
            let y = (flake.y * wh as f32 / 720.0).round() as i32 - sp.height as i32 / 2;
            frame.blit_surface_alpha(sp, x, y, flake.alpha);
        }
    }
}

/// Original title screen. The logo, highlight bar and copyright stay the
/// game's art. The five item labels are the system font, white at rest and
/// gold when selected, so a player font can replace them later.
///
///   T0000  1280x856  street plate. A 720-tall window pans down it.
///   weather.tga      snowfall, drawn over the street and the logo
///   T0011  752x448   logo (transparent; the backdrop is its edge colour #011D59)
///   T0101  320x80    highlight bar: two 320x40 frames (dim / bright)
///   T0020  264x24    copyright line
///
/// The original title adds 1.2px per frame to the plate's source Y and clamps
/// at 136 (856 − 720), so the street settles after about two seconds. The bar
/// slides between items and flashes its bright frame while it moves. Arrow
/// keys move, Space/Enter confirms, Esc quits; items whose subsystems do not
/// exist yet answer with a notice.
///
/// `WA2_TITLE_AUTO=<n>` pre-selects item `n` and confirms it (verification runs).
fn run_title(
    win: &mut sdl2::Sdl,
    font: &render::Font,
    bg_dirs: &[&std::path::Path],
    script_dir: &std::path::Path,
    cn: bool,
) -> TitleAction {
    const W: u32 = 1280;
    const H: u32 = 720;
    const ITEM_W: i32 = 320;
    const ITEM_H: i32 = 40;
    // The highlight oval is 40px. The labels are larger than the original
    // bitmap row, so the rows sit further apart than the oval height.
    const PITCH: i32 = 52;
    const N: usize = 5;
    const X0: i32 = (W as i32 - ITEM_W) / 2;
    const Y0: i32 = 412;
    const BACKDROP: [u8; 4] = [1, 29, 89, 255]; // T0011's edge colour

    let items: [&str; 5] = if cn {
        ["开始游戏", "继续游戏", "系统设置", "特别模式", "结束游戏"]
    } else {
        ["はじめから", "つづきから", "オプション", "スペシャル", "ゲーム終了"]
    };
    let notice_text = if cn { "此功能尚未实现" } else { "ただいま準備中です" };
    let mut label_px = 0u32;
    let mut labels = Vec::new();
    let mut note_px = 0u32;
    let mut note_img: Option<(u32, u32, Vec<u8>)> = None;
    let mut fitted: Option<FittedMenu> = None;

    let street = load_background(bg_dirs, "T0000.tga");
    let weather = load_background(bg_dirs, "weather.tga");
    let mut snow = TitleSnow::new();
    let logo = load_background(bg_dirs, "T0011.tga");
    let copyright = load_background(bg_dirs, "T0020.tga");
    let bar = load_background(bg_dirs, "T0101.tga");
    // Source Y of the 720-tall window into T0000. The exe adds 1.2 per frame
    // and stops at image_height − 720, which is 136 for the shipped plate.
    let scroll_max = street
        .as_ref()
        .map(|img| (img.height as i32 - H as i32).max(0) as f32)
        .unwrap_or(0.0);
    let mut scroll = 0.0f32;
    let mut scroll_tick = std::time::Instant::now();
    let mut street_scaled: Option<(i32, i32, render::Surface)> = None;

    if std::env::var_os("WA2_DEBUG_INPUT").is_some() {
        let (ww, wh) = win.window_size();
        eprintln!("title: window_size = {}x{}", ww, wh);
    }
    // 「画面模式」 applies as soon as the option screen is left.
    let startup = menu::Settings::load();
    win.set_fullscreen(startup.win_disp == 1);
    set_render_art_scale(win);
    let mixer = audio::Mixer::new(win);
    if let Some(ref mx) = mixer {
        menu::apply(&startup, Some(mx));
    }
    let cursor = cursor_se_path(bg_dirs);

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
                    play_cursor(mixer.as_ref(), cursor.as_deref());
                }
                sdl2::Input::Down => {
                    sel = (sel + 1) % N;
                    flash_until = std::time::Instant::now() + std::time::Duration::from_millis(160);
                    play_cursor(mixer.as_ref(), cursor.as_deref());
                }
                sdl2::Input::Advance | sdl2::Input::Right => confirm = true,
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
                            play_cursor(mixer.as_ref(), cursor.as_deref());
                        }
                        if pressed {
                            confirm = true;
                        }
                    }
                }
                _ => {}
            }
        }

        // ---- animate the highlight bar and the street ----------------------
        let (ww, wh) = win.window_size();
        let ww = ww.max(1);
        let wh = wh.max(1);
        slide_bar(&mut bar_y, item_y(sel), wh);
        let flashing = std::time::Instant::now() < flash_until;
        let now = std::time::Instant::now();
        let dt = now.saturating_duration_since(scroll_tick).as_secs_f32().min(0.05);
        scroll_tick = now;
        if scroll < scroll_max {
            // 1.2 layout pixels per frame at 60fps.
            scroll = (scroll + dt * 72.0).min(scroll_max);
        }
        if weather.is_some() {
            snow.step(dt);
        }

        // ---- compose --------------------------------------------------------
        // T0100 draws each label about 29px tall inside the 40px row.
        let ipx = layout_font_px(32, wh);
        if ipx != label_px {
            label_px = ipx;
            labels = menu_labels(&items, ipx);
        }
        if let Some(n) = notice {
            if cn {
                let px = layout_font_px(26, wh);
                if px != note_px {
                    note_px = px;
                    note_img =
                        render::text::with_ui_font(px, |f| f.line_rgba(n, px)).flatten();
                }
            }
        } else {
            note_px = 0;
        }
        let baked_notice = notice.is_some() && (!cn || note_img.is_none());

        ensure_fitted(
            &mut fitted,
            ww,
            wh,
            if baked_notice { 1 } else { 0 },
            || {
                // The street is composited under this plate. With no street
                // the plate itself is the navy field the logo was authored on.
                let plate_bg = if street.is_some() { [0, 0, 0, 0] } else { BACKDROP };
                let mut s = render::Surface::new(W, H, plate_bg);
                if let Some(ref lg) = logo {
                    s.blit(lg, (W as i32 - lg.width as i32) / 2, 24);
                }
                if baked_notice {
                    if let Some(n) = notice {
                        let cell = font.cell() as i32;
                        let wpx = n.chars().count() as i32 * cell;
                        let nx = W as i32 / 2 - wpx / 2;
                        s.fill_rect(
                            nx - 16,
                            372,
                            (wpx + 32) as u32,
                            (cell + 12) as u32,
                            [1, 29, 89, 235],
                        );
                        font.draw(&mut s, n, nx, 378);
                    }
                }
                if let Some(ref cp) = copyright {
                    s.blit(cp, W as i32 - cp.width as i32 - 24, H as i32 - cp.height as i32 - 14);
                }
                s
            },
            bar.as_ref(),
            ITEM_W as u32,
            ITEM_H as u32,
        );
        let fitted = fitted.as_ref().unwrap();
        let mut frame = if let Some(ref img) = street {
            let rebuild = match street_scaled.as_ref() {
                Some((sw, sh, _)) => *sw != ww || *sh != wh,
                None => true,
            };
            if rebuild {
                let dh = ((img.height as i64 * wh as i64) / H as i64).max(wh as i64) as u32;
                street_scaled = Some((ww, wh, surface_from_tga(img).scaled_to(ww as u32, dh)));
            }
            let scaled = &street_scaled.as_ref().unwrap().2;
            let max_y = (scaled.height as i32 - wh).max(0);
            let sy = ((scroll * wh as f32 / H as f32).round() as i32).clamp(0, max_y);
            let mut frame = render::Surface::new(ww as u32, wh as u32, BACKDROP);
            frame.copy_rows_from(scaled, sy);
            frame.blit_surface(&fitted.plate, 0, 0);
            frame
        } else {
            clone_surf(&fitted.plate)
        };
        // Bar under the labels: the glyphs are drawn on top of the oval.
        paint_bar(&mut frame, fitted, if flashing { 1 } else { 0 }, bar_y, X0);
        draw_menu_labels(&mut frame, &labels, sel, X0, Y0, ITEM_W, ITEM_H, PITCH, ww, wh);
        if notice.is_some() {
            if let Some((rw, rh, rgba)) = note_img.as_ref() {
                let nx = ww / 2 - *rw as i32 / 2;
                let ny = 370 * wh / 720;
                let pad_x = (16 * ww / 1280).max(8);
                let pad_y = (4 * wh / 720).max(2);
                frame.fill_rect(
                    nx - pad_x,
                    ny - pad_y,
                    *rw + pad_x as u32 * 2,
                    *rh + pad_y as u32 * 2,
                    [1, 29, 89, 235],
                );
                frame.blit_rgba(rgba, *rw, *rh, nx, ny);
            }
        }
        if let Some(ref sheet) = weather {
            snow.ensure(ww, wh, sheet);
            snow.paint(&mut frame, wh);
        }
        win.present(&frame);

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
                    set_render_art_scale(win);
                    notice = None;
                }
                3 => match run_special(
                    win,
                    bg_dirs,
                    cn,
                    mixer.as_ref(),
                    cursor.as_deref(),
                ) {
                    SpecialPick::Replay => {
                        if let Some(id) = timeline::run(win, font, bg_dirs, script_dir, cn) {
                            return TitleAction::Jump(id);
                        }
                        notice = None;
                    }
                    SpecialPick::Cg => {
                        let names = cg_names(bg_dirs);
                        if names.is_empty() {
                            notice = Some(notice_text);
                        } else {
                            browse_cg(win, font, bg_dirs, &names);
                            notice = None;
                        }
                    }
                    SpecialPick::Music => {
                        let bgm_dir = script_dir.parent().unwrap_or(std::path::Path::new(".")).join("bgm");
                        run_music(win, bg_dirs, &bgm_dir, cn, mixer.as_ref(), cursor.as_deref());
                        notice = None;
                    }
                    SpecialPick::Back => notice = None,
                    SpecialPick::Notice => notice = Some(notice_text),
                },
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
    println!("  wa2-native font-test <font.tga> <charset.txt> <out.tga> [text]  atlas glyph sheet");
    println!("  wa2-native font-sample <out.tga> [w] [h]  vector-font specimen sheet (WA2_FONT*)");
}

#[cfg(test)]
mod menu_bar {
    #[test]
    fn windowed_step_is_thirty_percent() {
        let mut y = 432.0;
        super::slide_bar(&mut y, 472.0, 720);
        assert!((y - 444.0).abs() < 0.01, "{y}");
    }

    #[test]
    fn maximized_step_stays_within_six_screen_pixels() {
        let mut y = 432.0;
        let mut prev = super::bar_window_y(y, 1080);
        for _ in 0..90 {
            super::slide_bar(&mut y, 472.0, 1080);
            let now = super::bar_window_y(y, 1080);
            assert!((now - prev).abs() <= 6, "{prev} -> {now}");
            prev = now;
            if y == 472.0 {
                break;
            }
        }
        assert_eq!(y, 472.0);
        // A fraction of a layout pixel still moves the bar on a tall window.
        assert_eq!(super::bar_window_y(432.0, 1080), 648);
        assert_eq!(super::bar_window_y(432.4, 1080), 649);
    }

    #[test]
    fn highlight_tracks_fractional_layout_y() {
        let rgba = vec![255u8; 320 * 40 * 4];
        let img = crate::codec::tga::TgaImage { width: 320, height: 40, rgba };
        let sprite = super::scale_sprite(&img, 0, 0, 320, 40, 480, 60);
        let fitted = super::FittedMenu {
            ww: 1920,
            wh: 1080,
            stamp: 0,
            plate: crate::render::Surface::new(1, 1, [0, 0, 0, 255]),
            bar: [Some(sprite), None],
        };
        let mut a = crate::render::Surface::new(1920, 1080, [1, 29, 89, 255]);
        let mut b = crate::render::Surface::new(1920, 1080, [1, 29, 89, 255]);
        super::paint_bar(&mut a, &fitted, 0, 432.0, 480);
        super::paint_bar(&mut b, &fitted, 0, 440.0, 480);
        let top = |s: &crate::render::Surface| -> i32 {
            for y in 0..s.height {
                for x in 0..s.width {
                    let i = ((y * s.width + x) * 4) as usize;
                    if s.pixels[i] > 200 {
                        return y as i32;
                    }
                }
            }
            -1
        };
        let dy = top(&b) - top(&a);
        assert!((dy - 12).abs() <= 1, "dy {dy}");
    }
}

#[cfg(test)]
mod rewind_history {
    #[test]
    fn rewind_drops_the_line_on_screen_and_keeps_the_previous() {
        let mut history = vec!["a", "b", "c"];
        assert_eq!(super::take_rewind(&mut history, true).as_deref(), Some("b"));
        assert_eq!(history, ["a", "b"]);
        assert_eq!(super::take_rewind(&mut history, true).as_deref(), Some("a"));
        assert_eq!(history, ["a"]);
        assert!(super::take_rewind(&mut history, true).is_none());
        assert_eq!(history, ["a"]);
        // A choice has not pushed a frame of its own, so the line stays put.
        assert_eq!(super::take_rewind(&mut history, false).as_deref(), Some("a"));
        assert_eq!(history, ["a"]);
    }
}

mod backlog_paint {
    #[test]
    fn history_paints_the_banner_and_the_lines() {
        let atlas = crate::codec::tga::decode_font(
            &std::fs::read("/home/lrxin/wa2-native/data/font/cn_body80.tga").unwrap(),
        )
        .unwrap();
        let font = crate::render::Font::from_cell_map(
            atlas,
            40,
            std::path::Path::new("/home/lrxin/wa2-native/data/font/wa2_cell_map_cn.tsv"),
        )
        .unwrap();
        let dirs = [
            std::path::Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            std::path::Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let header = super::load_background(&dirs, "sys_00032.tga");
        let scroll = super::load_background(&dirs, "sys_00030.tga");
        assert!(header.is_some());
        assert!(scroll.is_some());
        let scene = crate::render::Surface::new(1280, 720, [80, 120, 160, 255]);
        let lines = [
            "春希：不我".to_string(),
            "春希：不我".to_string(),
        ];
        let rows = super::backlog_rows(&lines, None);
        assert!(rows.len() > 2, "a name and a body should not collapse into one row");
        assert!(rows.iter().any(|r| r.name), "the speaker should be its own row");
        let per_page = 19;
        let top = rows.len().saturating_sub(per_page);
        let surf = super::paint_backlog(
            &scene,
            &font,
            None,
            &rows,
            top,
            per_page,
            header.as_ref(),
            scroll.as_ref(),
            false,
            false,
        );
        // Banner sits on the top edge and is not the flat scene colour.
        let i = (20 * 1280 + 200) * 4;
        assert!(surf.pixels[i + 2] > surf.pixels[i], "banner should be cyan, not grey");
        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/backlog.tga");
    }

    /// The history is the last screen that still reached for the atlas. In
    /// Chinese mode it draws with the vector font, and the two renders have to
    /// differ - that is the whole point of the change.
    #[test]
    fn history_paints_the_lines_with_the_vector_font() {
        let Some(mut vf) = crate::render::text::VectorFont::open(
            crate::render::text::DEFAULT_FONT,
            crate::render::DIALOG_FONT_720,
        ) else {
            return;
        };
        let dirs = [
            std::path::Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            std::path::Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let header = super::load_background(&dirs, "sys_00032.tga");
        let scroll = super::load_background(&dirs, "sys_00030.tga");
        let scene = crate::render::Surface::new(1280, 720, [80, 120, 160, 255]);
        let lines = vec![
            "春希：不我".to_string(),
            "冬马和纱：白色的季节里".to_string(),
        ];
        let rows = super::backlog_rows(&lines, Some(&vf));
        assert!(rows.iter().any(|r| r.name), "speaker row is missing");
        let painted = super::paint_backlog(
            &scene,
            &crate::render::Font::from_cell_map(
                crate::codec::tga::decode_font(
                    &std::fs::read("/home/lrxin/wa2-native/data/font/cn_body80.tga").unwrap(),
                )
                .unwrap(),
                40,
                std::path::Path::new("/home/lrxin/wa2-native/data/font/wa2_cell_map_cn.tsv"),
            )
            .unwrap(),
            Some(&mut vf),
            &rows,
            0,
            19,
            header.as_ref(),
            scroll.as_ref(),
            false,
            false,
        );
        // Ink where the lines are, in the text column and below the banner.
        let mut ink = 0usize;
        for y in 52..680 {
            for x in 72..1160 {
                let i = ((y * 1280 + x) * 4) as usize;
                if painted.pixels[i] > 200 {
                    ink += 1;
                }
            }
        }
        assert!(ink > 200, "vector history drew no glyphs ({ink} px)");
        let _ = painted.save_tga("/tmp/wa2btn/backlog_vector.tga");
    }

    #[test]
    fn quick_load_confirm_paints_yes_and_no() {
        let dirs = [
            std::path::Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            std::path::Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let plate = super::load_background(&dirs, "sys_00100.tga").unwrap();
        let buttons = super::load_background(&dirs, "sys_00101.tga").unwrap();
        let scene = crate::render::Surface::new(1280, 720, [20, 20, 20, 255]);
        let text = crate::render::text::with_ui_font(22, |f| {
            f.line_rgba("要读取快速存档吗？", 22)
        })
        .flatten();
        assert!(text.is_some(), "system font should rasterize the prompt");
        let surf = super::paint_confirm(&scene, Some(&plate), Some(&buttons), text.as_ref(), Some(true));
        let (px, py) = super::centered_plate(plate.width as i32, plate.height as i32);
        let boxes = super::ask_buttons(px, py, plate.width as i32, plate.height as i32);
        let sample = |x: i32, y: i32| {
            let i = ((y as u32 * 1280 + x as u32) * 4) as usize;
            surf.pixels[i] as u16 + surf.pixels[i + 1] as u16 + surf.pixels[i + 2] as u16
        };
        let yes = sample(boxes[0].0 + 80, boxes[0].1 + 20);
        let no = sample(boxes[1].0 + 80, boxes[1].1 + 20);
        assert!(yes > 400, "yes button should be the gold hover row, sum {yes}");
        assert!(no > 400, "no button should still be the idle row, sum {no}");
        assert!(yes != no, "hover should change the yes button");
        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/confirm.tga");
    }
}
