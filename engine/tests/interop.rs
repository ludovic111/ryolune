//! Working with other apps: DAWproject both ways (a ryolune song survives the round trip; a
//! project in the shape Bitwig Studio writes opens with a report of what changed), MIDI and
//! audio stems as new songs, the export formats, the apps table, the first-run setup and the
//! recent songs.
use ryolune_engine::{
    audio::{AudioBuffer, Library},
    automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation},
    control::{self, Headless, Host},
    host::scan,
    interop::{self, dawproject},
    model::*,
    settings::Settings,
    store,
    tempo::TempoPoint,
};
use serde_json::{json, Value};
use std::{io::Write, path::Path, sync::Arc};

fn run(host: &mut Headless, name: &str, params: Value) -> Value {
    control::call(host, name, &params, false).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn tone(seconds: f64, rate: u32) -> Arc<AudioBuffer> {
    let frames = (seconds * rate as f64) as usize;
    Arc::new(
        AudioBuffer::new(
            rate,
            (0..frames)
                .map(|i| {
                    let v = (i as f32 * 0.05).sin() * 0.25;
                    [v, -v]
                })
                .collect(),
        )
        .unwrap(),
    )
}

fn wav(seconds: f64, rate: u32) -> Vec<u8> {
    ryolune_engine::audio::encode_wav(&tone(seconds, rate)).unwrap()
}

/// A song with everything DAWproject carries: an instrument track on a stock instrument
/// with a stock effect, notes and controllers; an audio track with an offset, fades and a
/// send; both routed to a group bus; markers, a tempo ramp and a step, volume and pan
/// automation, a 3/4 meter.
fn rich_song() -> (Session, Library) {
    let mut s = interop::blank("Round trip");
    s.transport.time_signature = TimeSignature {
        numerator: 3,
        denominator: 4,
    };
    s.transport.tempo = 96.0;
    s.tempo_changes = vec![
        TempoPoint {
            bar: 4.0,
            bpm: 120.0,
            ramp: true,
        },
        TempoPoint {
            bar: 8.0,
            bpm: 90.0,
            ramp: false,
        },
    ];
    let track = |id: &str, name: &str, kind: &str, color: &str| Track {
        id: id.into(),
        name: name.into(),
        color: color.into(),
        armed: false,
        monitor: Monitor::Off,
        extra: Default::default(),
        kind: kind.into(),
        volume: 0.75,
        pan: 0.0,
        mute: false,
        solo: false,
        output: None,
    };
    let mut keys = track("keys", "Keys", "midi", "#ed835e");
    keys.volume = 0.6;
    keys.pan = -40.0;
    keys.output = Some("group".into());
    let mut drums = track("drums", "Drum loop", "audio", "#6ab3fd");
    drums.mute = true;
    drums.solo = true;
    drums.volume = 0.9;
    drums.output = Some("group".into());
    let group = track("group", "Group", "bus", "#95bd69");
    s.tracks = vec![keys, drums, group];
    let mut comp = Insert::new("comp-1".into(), "stock:ryolune Comp", "ryolune Comp");
    comp.params.insert(0, -18.0);
    comp.params.insert(1, 3.5);
    s.strips.insert(
        "keys".into(),
        Strip {
            instrument: "Analog Bass".into(),
            inserts: vec![comp],
            ..Default::default()
        },
    );
    s.strips.insert(
        "drums".into(),
        Strip {
            sends: vec![
                Send {
                    level_db: Some(-12.0),
                    name: String::new(),
                    bus: None,
                },
                Send {
                    level_db: None,
                    name: String::new(),
                    bus: None,
                },
            ],
            ..Default::default()
        },
    );
    let notes = vec![
        Note {
            id: "n1".into(),
            start: 0.0,
            length: 1.0,
            pitch: 60,
            velocity: 100,
            agent: false,
            channel: 0,
        },
        Note {
            id: "n2".into(),
            start: 1.5,
            length: 0.25,
            pitch: 64,
            velocity: 37,
            agent: false,
            channel: 2,
        },
    ];
    let controllers = vec![
        Controller {
            id: "c1".into(),
            kind: ControllerKind::Cc,
            number: Some(74),
            time: 0.5,
            value: 99,
            agent: false,
            channel: 0,
        },
        Controller {
            id: "c2".into(),
            kind: ControllerKind::Bend,
            number: None,
            time: 1.0,
            value: -4096,
            agent: false,
            channel: 0,
        },
        Controller {
            id: "c3".into(),
            kind: ControllerKind::PolyPressure,
            number: Some(64),
            time: 1.75,
            value: 50,
            agent: false,
            channel: 2,
        },
    ];
    s.clips.push(Clip {
        id: "k1".into(),
        name: "Chords".into(),
        agent: false,
        track_id: "keys".into(),
        start_bar: 1.0,
        length_bars: 2.0,
        data: ClipData::Midi { notes, controllers },
    });
    let buffer = tone(3.0, 44100);
    s.sources.insert(
        "loop".into(),
        Source {
            id: "loop".into(),
            name: "Loop".into(),
            sample_rate: 44100,
            channels: 2,
            file_name: Some("loop.wav".into()),
            duration_seconds: buffer.duration(),
            origin: "file".into(),
            seed: None,
            wave_kind: None,
        },
    );
    s.clips.push(Clip {
        id: "a1".into(),
        name: "Loop".into(),
        agent: false,
        track_id: "drums".into(),
        start_bar: 2.0,
        length_bars: 1.0,
        data: ClipData::Audio {
            source_id: "loop".into(),
            offset_seconds: 0.5,
            fade_in: 0.1,
            fade_out: 0.25,
            fade_curve: FadeCurve::EqualPower,
            gain_db: 0.0,
        },
    });
    s.markers = vec![
        Marker {
            id: "m1".into(),
            bar: 0.0,
            name: "Intro".into(),
            color: None,
        },
        Marker {
            id: "m2".into(),
            bar: 4.0,
            name: "Drop".into(),
            color: Some("#ff8800".into()),
        },
    ];
    s.automation.push(AutomationLane {
        id: "lane-1".into(),
        name: "Volume".into(),
        target: AutomationTarget::TrackVolume {
            track_id: "keys".into(),
        },
        min: 0.0,
        max: 1.0,
        manual_value: 0.6,
        interpolation: Interpolation::Linear,
        enabled: true,
        points: vec![
            AutomationPoint {
                id: "p1".into(),
                beat: 0.0,
                value: 0.2,
            },
            AutomationPoint {
                id: "p2".into(),
                beat: 6.0,
                value: 0.75,
            },
        ],
    });
    s.automation.push(AutomationLane {
        id: "lane-2".into(),
        name: "Pan".into(),
        target: AutomationTarget::TrackPan {
            track_id: "drums".into(),
        },
        min: -100.0,
        max: 100.0,
        manual_value: 0.0,
        interpolation: Interpolation::Step,
        enabled: true,
        points: vec![AutomationPoint {
            id: "p3".into(),
            beat: 3.0,
            value: 50.0,
        }],
    });
    s.master_volume = 0.7;
    s.normalize();
    s.validate().unwrap();
    let mut library = Library::new();
    library.insert("loop".into(), buffer);
    (s, library)
}

fn by_name<'a>(s: &'a Session, name: &str) -> &'a Track {
    s.tracks.iter().find(|t| t.name == name).unwrap_or_else(|| {
        panic!(
            "no track {name}: {:?}",
            s.tracks.iter().map(|t| &t.name).collect::<Vec<_>>()
        )
    })
}
fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn a_ryolune_song_survives_the_round_trip_through_dawproject() {
    let (song, library) = rich_song();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Round trip.dawproject");
    let report = dawproject::export(&song, &library, &path).unwrap();
    assert_eq!(report.format, "dawproject");
    assert!(
        report.kept.iter().any(|l| l.contains("1 instrument track")),
        "{report:?}"
    );
    assert!(
        report
            .approximated
            .iter()
            .any(|l| l.contains("named devices")),
        "stock devices are said to be ryolune's: {report:?}"
    );
    let imported = interop::import(&[path.clone()], &scan::installed()).unwrap();
    let back = imported.session;
    let r = &imported.report;
    assert!(
        r.kept.iter().any(|l| l.starts_with("Read from ryolune")),
        "{r:?}"
    );
    assert!(r.dropped.is_empty() && r.missing_media.is_empty(), "{r:?}");

    // Tempo map, meter and markers.
    assert_eq!(back.transport.tempo, 96.0);
    assert_eq!(back.transport.time_signature.numerator, 3);
    assert_eq!(back.tempo_changes, song.tempo_changes);
    assert_eq!(
        back.markers
            .iter()
            .map(|m| (m.bar, m.name.as_str(), m.color.clone()))
            .collect::<Vec<_>>(),
        vec![(0.0, "Intro", None), (4.0, "Drop", Some("#ff8800".into()))]
    );

    // Tracks, in order, with their mix and routing.
    assert_eq!(
        back.tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind.as_str(), t.color.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("Keys", "midi", "#ed835e"),
            ("Drum loop", "audio", "#6ab3fd"),
            ("Group", "bus", "#95bd69")
        ]
    );
    let keys = by_name(&back, "Keys");
    let drums = by_name(&back, "Drum loop");
    let group = by_name(&back, "Group");
    assert!(close(keys.volume as f64, 0.6, 1e-5) && close(keys.pan as f64, -40.0, 1e-4));
    assert!(drums.mute && drums.solo && close(drums.volume as f64, 0.9, 1e-5));
    assert_eq!(keys.output.as_deref(), Some(group.id.as_str()));
    assert_eq!(drums.output.as_deref(), Some(group.id.as_str()));
    assert!(close(back.master_volume as f64, 0.7, 1e-5));

    // Strips: the instrument, the effect with its settings, the sends.
    let keys_strip = &back.strips[&keys.id];
    assert_eq!(keys_strip.instrument, "Analog Bass");
    assert!(keys_strip.synth.is_none());
    assert_eq!(keys_strip.inserts.len(), 1);
    assert_eq!(keys_strip.inserts[0].plugin_id(), "stock:ryolune Comp");
    assert_eq!(keys_strip.inserts[0].params.get(&0), Some(&-18.0));
    assert_eq!(keys_strip.inserts[0].params.get(&1), Some(&3.5));
    let drum_sends = &back.strips[&drums.id].sends;
    assert_eq!(drum_sends.len(), 2);
    assert_eq!(drum_sends[0].target(0), Some(BUS_A));
    assert!(close(drum_sends[0].level_db.unwrap() as f64, -12.0, 1e-3));
    assert_eq!(drum_sends[1].target(1), Some(BUS_B));
    assert_eq!(drum_sends[1].level_db, None);
    assert_eq!(
        back.strips[BUS_A].inserts[0].plugin_id(),
        "stock:Space",
        "the aux returns come back in place"
    );

    // Clips, notes and controllers.
    let midi = back.clips.iter().find(|c| c.track_id == keys.id).unwrap();
    assert_eq!(
        (midi.name.as_str(), midi.start_bar, midi.length_bars),
        ("Chords", 1.0, 2.0)
    );
    let ClipData::Midi { notes, controllers } = &midi.data else {
        panic!("a MIDI clip")
    };
    let notes: Vec<_> = notes
        .iter()
        .map(|n| (n.start, n.length, n.pitch, n.velocity, n.channel))
        .collect();
    assert_eq!(notes, vec![(0.0, 1.0, 60, 100, 0), (1.5, 0.25, 64, 37, 2)]);
    let mut controllers: Vec<_> = controllers
        .iter()
        .map(|c| (c.kind, c.number, c.time, c.value, c.channel))
        .collect();
    controllers.sort_by(|a, b| a.2.total_cmp(&b.2));
    assert_eq!(
        controllers,
        vec![
            (ControllerKind::Cc, Some(74), 0.5, 99, 0),
            (ControllerKind::Bend, None, 1.0, -4096, 0),
            (ControllerKind::PolyPressure, Some(64), 1.75, 50, 2),
        ]
    );
    let audio = back.clips.iter().find(|c| c.track_id == drums.id).unwrap();
    assert!(close(audio.start_bar, 2.0, 1e-9) && close(audio.length_bars, 1.0, 1e-9));
    let ClipData::Audio {
        source_id,
        offset_seconds,
        fade_in,
        fade_out,
        ..
    } = &audio.data
    else {
        panic!("an audio clip")
    };
    assert!(close(*offset_seconds, 0.5, 1e-9));
    assert!(close(*fade_in, 0.1, 1e-9) && close(*fade_out, 0.25, 1e-9));
    let buffer = &imported.library[source_id];
    assert_eq!(buffer.frames.len(), library["loop"].frames.len());
    assert_eq!(buffer.sample_rate, 44100);
    assert_eq!(back.sources[source_id].name, "Loop");

    // Automation.
    let volume = back
        .automation
        .iter()
        .find(|l| {
            l.target
                == AutomationTarget::TrackVolume {
                    track_id: keys.id.clone(),
                }
        })
        .unwrap();
    assert_eq!(volume.interpolation, Interpolation::Linear);
    let points: Vec<_> = volume.points.iter().map(|p| (p.beat, p.value)).collect();
    assert_eq!(points.len(), 2);
    assert!(close(points[0].1, 0.2, 1e-6) && close(points[1].1, 0.75, 1e-6) && points[1].0 == 6.0);
    let pan = back
        .automation
        .iter()
        .find(|l| {
            l.target
                == AutomationTarget::TrackPan {
                    track_id: drums.id.clone(),
                }
        })
        .unwrap();
    assert_eq!(pan.interpolation, Interpolation::Step);
    assert!(close(pan.points[0].value, 50.0, 1e-6) && pan.points[0].beat == 3.0);
}

#[test]
fn the_project_xml_follows_the_schema_order_and_names_plugins_by_id() {
    let (mut song, library) = rich_song();
    // An external plugin with state: a VST3 effect and a CLAP instrument.
    let mut vst = Insert::new(
        "v1".into(),
        "vst3:0123456789abcdef0123456789abcdef",
        "Pro-Q 3",
    );
    let mut blob = vec![];
    blob.extend_from_slice(&4u32.to_le_bytes());
    blob.extend_from_slice(b"comp");
    blob.extend_from_slice(&4u32.to_le_bytes());
    blob.extend_from_slice(b"cont");
    vst.blob = ryolune_engine::host::encode_blob(&blob);
    song.strips.get_mut("drums").unwrap().inserts.push(vst);
    let mut clap = Insert::new(
        "c1".into(),
        "clap:org.surge-synth-team.surge-xt",
        "Surge XT",
    );
    clap.blob = ryolune_engine::host::encode_blob(b"raw clap state");
    clap.state = "bypassed".into();
    song.strips.get_mut("keys").unwrap().synth = Some(clap);
    let written = dawproject::write(&song, &library).unwrap();
    let xml = &written.project;
    let doc = roxmltree_check(xml);
    assert!(doc.contains("<Application name=\"ryolune\""));
    assert!(xml.contains("deviceID=\"01234567-89AB-CDEF-0123-456789ABCDEF\""));
    assert!(xml.contains("<ClapPlugin deviceID=\"org.surge-synth-team.surge-xt\""));
    assert!(xml.contains("role=\"submix\""), "the group is a submix");
    assert!(
        xml.contains("role=\"effect\""),
        "the aux returns are effect channels"
    );
    assert!(xml.contains("role=\"master\""));
    // Channel children in the schema's order: Devices, Mute, Pan, Sends, Volume.
    let channel = &xml[xml.find("<Channel").unwrap()..xml.find("</Channel>").unwrap()];
    let order: Vec<usize> = ["<Devices>", "<Mute ", "<Pan ", "<Volume "]
        .iter()
        .map(|tag| channel.find(tag).unwrap_or_else(|| panic!("{tag}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{channel}");
    let names: Vec<&str> = written.files.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"audio/Loop.wav"), "{names:?}");
    assert!(names.iter().any(|n| n.ends_with(".vstpreset")));
    assert!(names.iter().any(|n| n.ends_with(".clap-preset")));
    let preset = &written
        .files
        .iter()
        .find(|(n, _)| n.ends_with(".vstpreset"))
        .unwrap()
        .1;
    assert_eq!(&preset[0..4], b"VST3");
    let clap_state = &written
        .files
        .iter()
        .find(|(n, _)| n.ends_with(".clap-preset"))
        .unwrap()
        .1;
    assert_eq!(clap_state.as_slice(), b"raw clap state");
    assert!(written.metadata.contains("<Title>Round trip</Title>"));

    // Back in ryolune with the plugins installed: they load with their state.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugins.dawproject");
    dawproject::export(&song, &library, &path).unwrap();
    let mut catalog = scan::installed();
    for (id, name) in [
        ("vst3:0123456789abcdef0123456789abcdef", "Pro-Q 3"),
        ("clap:org.surge-synth-team.surge-xt", "Surge XT"),
    ] {
        catalog.push(ryolune_engine::plugin::Descriptor {
            id: id.into(),
            format: if id.starts_with("vst3") {
                ryolune_engine::plugin::Format::Vst3
            } else {
                ryolune_engine::plugin::Format::Clap
            },
            name: name.into(),
            vendor: String::new(),
            path: String::new(),
            instrument: id.starts_with("clap"),
            effect: !id.starts_with("clap"),
            category: String::new(),
        });
    }
    let imported = interop::import(&[path.clone()], &catalog).unwrap();
    let s = &imported.session;
    let keys = by_name(s, "Keys");
    let synth = s.strips[&keys.id].synth.as_ref().unwrap();
    assert_eq!(synth.plugin, "clap:org.surge-synth-team.surge-xt");
    assert_eq!(synth.state, "bypassed");
    assert_eq!(
        ryolune_engine::host::decode_blob(&synth.blob).unwrap(),
        b"raw clap state"
    );
    let drums = by_name(s, "Drum loop");
    let vst = &s.strips[&drums.id].inserts[0];
    assert_eq!(vst.plugin, "vst3:0123456789abcdef0123456789abcdef");
    assert_eq!(ryolune_engine::host::decode_blob(&vst.blob).unwrap(), blob);

    // Without them installed, they are left out and said so.
    let imported = interop::import(&[path], &scan::installed()).unwrap();
    let r = &imported.report;
    assert!(
        r.dropped
            .iter()
            .any(|l| l.contains("Surge XT (CLAP) on Keys: not installed here")),
        "{r:?}"
    );
    let keys = by_name(&imported.session, "Keys");
    assert!(keys.extra["importNotes"][0]
        .as_str()
        .unwrap()
        .contains("Surge XT"));
    assert_eq!(
        imported.session.strips[&keys.id].instrument,
        "ryolune Synth"
    );
}

/// Parse with the same XML reader the import uses, so a malformed file fails here.
fn roxmltree_check(xml: &str) -> String {
    let doc = roxmltree::Document::parse(xml).expect("well-formed XML");
    assert_eq!(doc.root_element().tag_name().name(), "Project");
    xml.to_string()
}

fn bitwig_fixture(dir: &Path) -> std::path::PathBuf {
    let project = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dawproject/project.xml"),
    )
    .unwrap();
    let path = dir.join("Bitwig song.dawproject");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("metadata.xml", options).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<MetaData><Title>Night Drive</Title><Artist>Someone</Artist></MetaData>")
        .unwrap();
    zip.start_file("project.xml", options).unwrap();
    zip.write_all(project.as_bytes()).unwrap();
    zip.start_file("audio/Drumfunk3 170bpm.wav", options)
        .unwrap();
    zip.write_all(&wav(2.823541666666667, 48000)).unwrap();
    zip.finish().unwrap();
    path
}

#[test]
fn a_project_in_bitwig_shape_opens_with_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let path = bitwig_fixture(dir.path());
    let imported = interop::import(&[path], &scan::installed()).unwrap();
    let s = &imported.session;
    let r = &imported.report;
    assert_eq!(s.name, "Night Drive", "the title comes from metadata.xml");
    assert!(
        r.kept.contains(&"Read from Bitwig Studio 5.0".to_string()),
        "{r:?}"
    );

    // Tempo 149 gliding to 160 at beat 16 (bar 4), 4/4.
    assert_eq!(s.transport.tempo, 149.0);
    assert_eq!(
        s.tempo_changes,
        vec![TempoPoint {
            bar: 4.0,
            bpm: 160.0,
            ramp: true
        }]
    );

    // Tracks: the Reverb effect channel is a bus; the master's volume is the Stereo Out's.
    assert_eq!(
        s.tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![("Bass", "midi"), ("Drumloop", "audio"), ("Reverb", "bus")]
    );
    let bass = by_name(s, "Bass");
    let drums = by_name(s, "Drumloop");
    let reverb = by_name(s, "Reverb");
    assert_eq!(bass.color, "#a2eabf");
    assert!(close(fader_gain(bass.volume) as f64, 0.659140, 1e-4));
    assert!(close(bass.pan as f64, -50.0, 1e-3));
    assert!(drums.mute && drums.solo);
    assert!(close(s.master_volume as f64, 0.75, 1e-5));
    let send = &s.strips[&bass.id].sends[0];
    assert_eq!(send.bus.as_deref(), Some(reverb.id.as_str()));
    assert!(close(send.level_db.unwrap() as f64, -6.0206, 1e-3));

    // Surge XT is not installed: left out, said so, and the track plays ryolune Synth.
    assert!(
        r.dropped
            .iter()
            .any(|l| l.starts_with("Surge XT (CLAP, Surge Synth Team) on Bass")),
        "{r:?}"
    );
    assert_eq!(s.strips[&bass.id].instrument, "ryolune Synth");
    // Bitwig's own compressor becomes ryolune's.
    assert_eq!(
        s.strips[&drums.id].inserts[0].plugin_id(),
        "stock:ryolune Comp"
    );
    assert!(
        r.approximated
            .iter()
            .any(|l| l.contains("became ryolune's ryolune Comp")),
        "{r:?}"
    );

    // The bass clip: nine notes at velocity 100 and its mod wheel.
    let clip = s.clips.iter().find(|c| c.track_id == bass.id).unwrap();
    assert_eq!(
        (clip.name.as_str(), clip.start_bar, clip.length_bars),
        ("Bassline", 0.0, 2.0)
    );
    let ClipData::Midi { notes, controllers } = &clip.data else {
        panic!("MIDI")
    };
    assert_eq!(notes.len(), 9);
    assert!(notes.iter().all(|n| n.velocity == 100));
    assert_eq!(notes[0].pitch, 65);
    let long = notes.iter().find(|n| n.start == 1.5).unwrap();
    assert_eq!((long.pitch, long.length), (53, 2.5));
    assert_eq!(
        controllers
            .iter()
            .map(|c| (c.kind, c.number, c.time, c.value))
            .collect::<Vec<_>>(),
        vec![
            (ControllerKind::Cc, Some(1), 0.0, 0),
            (ControllerKind::Cc, Some(1), 4.0, 127)
        ]
    );

    // The drum loop: Bitwig's clip in a clip, warped, with its audio from the zip.
    let audio = s.clips.iter().find(|c| c.track_id == drums.id).unwrap();
    assert_eq!(audio.name, "Drumfunk3 170bpm");
    assert!(close(audio.length_bars, 2.0, 1e-4));
    let ClipData::Audio {
        source_id,
        offset_seconds,
        ..
    } = &audio.data
    else {
        panic!("audio")
    };
    assert_eq!(*offset_seconds, 0.0);
    assert!(close(imported.library[source_id].duration(), 2.8235, 1e-3));
    assert!(
        r.approximated.iter().any(|l| l.contains("time-stretched")),
        "{r:?}"
    );

    // Markers, volume automation; the launcher clip is named as left out.
    assert_eq!(
        s.markers
            .iter()
            .map(|m| (m.bar, m.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0.0, "Verse"), (2.0, "Chorus")]
    );
    assert_eq!(s.markers[1].color.as_deref(), Some("#ff8800"));
    let lane = s
        .automation
        .iter()
        .find(|l| {
            l.target
                == AutomationTarget::TrackVolume {
                    track_id: bass.id.clone(),
                }
        })
        .unwrap();
    assert_eq!(lane.points.len(), 2);
    assert!(close(lane.points[1].value, 0.75, 1e-6), "gain 1.0 is unity");
    assert!(
        r.dropped.iter().any(|l| l.contains("clip launcher")),
        "{r:?}"
    );
    s.validate().unwrap();
}

#[test]
fn import_from_replaces_the_song_and_export_to_picks_the_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = bitwig_fixture(dir.path());
    let mut host = Headless::new();
    let reply = run(
        &mut host,
        "project.importFrom",
        json!({ "path": path.to_string_lossy() }),
    );
    assert_eq!(reply["report"]["format"], "dawproject");
    assert_eq!(host.store.session().name, "Night Drive");
    assert!(host.store.dirty(), "an imported song is unsaved");
    assert!(host.path.is_none());
    assert!(!host.store.can_undo(), "it is a new document");

    // Writing it for an app: the app picks DAWproject or the MIDI-and-stems folder.
    let out = dir.path().join("For Cubase.dawproject");
    let reply = run(
        &mut host,
        "session.exportTo",
        json!({ "path": out.to_string_lossy(), "app": "cubase" }),
    );
    assert_eq!(reply["format"], "dawproject");
    assert!(out.is_file());
    let again = interop::import(&[out], &scan::installed()).unwrap();
    assert_eq!(again.session.tracks.len(), 3);

    let folder = dir.path().join("For Logic");
    let reply = run(
        &mut host,
        "session.exportTo",
        json!({ "path": folder.to_string_lossy(), "app": "logic" }),
    );
    assert_eq!(reply["format"], "package");
    assert!(folder.join("For Logic.mid").is_file());
    assert!(std::fs::read_dir(folder.join("Stems")).unwrap().count() >= 2);
    let err = control::call(
        &mut host,
        "session.exportTo",
        &json!({ "path": folder.to_string_lossy(), "app": "logic" }),
        false,
    )
    .unwrap_err();
    assert!(err.contains("already exists"), "{err}");

    let midi = dir.path().join("notes.mid");
    let reply = run(
        &mut host,
        "export.midi",
        json!({ "path": midi.to_string_lossy() }),
    );
    assert!(reply["noteCount"].as_u64().unwrap() > 0);
    let err = control::call(
        &mut host,
        "session.exportTo",
        &json!({ "path": dir.path().join("x.doc").to_string_lossy() }),
        false,
    )
    .unwrap_err();
    assert!(err.contains("give `format`"), "{err}");
}

#[test]
fn midi_files_and_stems_open_as_new_songs() {
    let dir = tempfile::tempdir().unwrap();
    // A MIDI file written by ryolune from the demo comes back with its tempo.
    let mut host = Headless::new();
    host.store.load(store::demo()).unwrap();
    let midi = dir.path().join("Nightfall.mid");
    run(
        &mut host,
        "session.exportMidi",
        json!({ "path": midi.to_string_lossy() }),
    );
    let imported = interop::import(&[midi], &scan::installed()).unwrap();
    assert_eq!(imported.report.format, "midi");
    assert_eq!(imported.session.name, "Nightfall");
    assert_eq!(
        imported.session.transport.tempo,
        store::demo().transport.tempo
    );
    assert!(imported.session.tracks.iter().all(|t| t.kind == "midi"));
    assert!(!imported.session.clips.is_empty());

    // Stems: one audio track per file, from bar 1.
    let a = dir.path().join("Drums.wav");
    let b = dir.path().join("Bass.flac.wav");
    std::fs::write(&a, wav(1.0, 48000)).unwrap();
    std::fs::write(&b, wav(2.0, 44100)).unwrap();
    let mut host = Headless::new();
    let reply = run(
        &mut host,
        "session.importFrom",
        json!({ "paths": [a.to_string_lossy(), b.to_string_lossy()] }),
    );
    assert_eq!(reply["report"]["format"], "audio");
    let s = host.store.session();
    assert_eq!(
        s.tracks
            .iter()
            .map(|t| (t.name.as_str(), t.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![("Drums", "audio"), ("Bass.flac", "audio")]
    );
    assert!(s.clips.iter().all(|c| c.start_bar == 0.0));
    assert_eq!(host.library.len(), 2);

    // Anything else says what ryolune opens.
    let doc = dir.path().join("song.als");
    std::fs::write(&doc, b"not a song").unwrap();
    let err = interop::import(&[doc], &[]).err().unwrap();
    assert!(err.contains("DAWproject (.dawproject)"), "{err}");
    let err = interop::import(&[a.clone(), dir.path().join("x.mid")], &[])
        .err()
        .unwrap();
    assert!(err.contains("audio stems only"), "{err}");
    // A zip that is not a DAWproject.
    let fake = dir.path().join("fake.dawproject");
    std::fs::write(&fake, b"PK nothing").unwrap();
    assert!(interop::import(&[fake], &[]).is_err());
}

#[test]
fn formats_list_every_app_and_the_aliases_reach_them() {
    let mut host = Headless::new();
    let reply = run(&mut host, "project.formats", json!({}));
    let apps: Vec<&str> = reply["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        apps,
        vec![
            "ableton",
            "logic",
            "fl",
            "bitwig",
            "reaper",
            "cubase",
            "studioone",
            "protools",
            "garageband"
        ]
    );
    assert!(reply["apps"][0]["installed"].is_boolean());
    let formats: Vec<&str> = reply["formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        formats,
        vec!["dawproject", "midi", "audio", "stems", "package"]
    );
    let one = run(&mut host, "session.formats", json!({ "app": "bitwig" }));
    assert_eq!(one["apps"].as_array().unwrap().len(), 1);
    assert_eq!(one["apps"][0]["writes"][0], "dawproject");
    assert!(control::call(
        &mut host,
        "session.formats",
        &json!({ "app": "nuendo" }),
        false
    )
    .is_err());
}

/// One test for both: they share the sandbox's settings file.
#[test]
fn the_first_run_setup_and_the_recent_songs_live_in_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert!(
        !Settings::read(&path).unwrap().onboarding.is_done(),
        "a first start"
    );
    std::fs::write(&path, r#"{"general":{"checkUpdatesOnStart":false}}"#).unwrap();
    assert!(
        Settings::read(&path).unwrap().onboarding.is_done(),
        "settings from before the setup existed"
    );
    std::fs::write(&path, r#"{"onboarding":{"completed":""}}"#).unwrap();
    assert!(!Settings::read(&path).unwrap().onboarding.is_done());

    let mut host = Headless::new();
    let state = run(&mut host, "app.onboarding", json!({}));
    assert_eq!(state["steps"][0]["id"], "comingFrom");
    let err = control::call(
        &mut host,
        "app.finishOnboarding",
        &json!({ "comingFrom": "logic", "ai": true }),
        true,
    )
    .unwrap_err();
    assert!(err.contains("not allowed for agents"), "{err}");
    let denied = ryolune_engine::control_app::denied_for_agent_request(
        "app.finishOnboarding",
        &json!({}),
        &Settings::default().agent.permissions,
    );
    assert!(denied.is_some());
    let state = run(
        &mut host,
        "app.finishOnboarding",
        json!({ "comingFrom": "ableton", "ai": false }),
    );
    assert_eq!(state["done"], true);
    assert_eq!(state["comingFrom"], "ableton");
    assert!(state["bring"]
        .as_str()
        .unwrap()
        .contains("All Individual Tracks"));
    assert_eq!(host.settings().onboarding.ai, Some(false));

    // Recent songs.
    host.store.load(store::demo()).unwrap();
    let song = dir.path().join("Kept.ryolune");
    run(
        &mut host,
        "session.save",
        json!({ "path": song.to_string_lossy() }),
    );
    let gone = dir.path().join("Gone.ryolune");
    let mut settings = host.settings();
    settings.general.recent_sessions = vec![
        song.to_string_lossy().into_owned(),
        gone.to_string_lossy().into_owned(),
    ];
    host.update_settings(settings).unwrap();
    let list = run(&mut host, "app.recent", json!({}));
    assert_eq!(list["recent"][0]["name"], "Kept");
    assert_eq!(list["recent"][0]["exists"], true);
    assert_eq!(list["recent"][1]["exists"], false);
    run(&mut host, "session.new", json!({}));
    run(&mut host, "app.openRecent", json!({ "index": 0 }));
    assert_eq!(host.path.as_deref(), Some(song.as_path()));
    let err =
        control::call(&mut host, "app.openRecent", &json!({ "index": 1 }), false).unwrap_err();
    assert!(err.contains("no longer there"), "{err}");
    let err = control::call(
        &mut host,
        "app.openRecent",
        &json!({ "path": "/elsewhere/x.ryolune" }),
        false,
    )
    .unwrap_err();
    assert!(err.contains("not in the recent songs"), "{err}");
    host.update_settings(Settings::default()).unwrap();
}

#[test]
fn a_song_from_the_demo_survives_dawproject_whole() {
    // The bundled demo uses generated audio and every stock voice: its notes and clips
    // come back where they were.
    let demo = store::demo();
    let mut library = Library::new();
    ryolune_engine::audio::prepare_sources(&demo, &mut library).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.dawproject");
    dawproject::export(&demo, &library, &path).unwrap();
    let back = interop::import(&[path], &scan::installed())
        .unwrap()
        .session;
    assert_eq!(back.tracks.len(), demo.tracks.len());
    assert_eq!(back.clips.len(), demo.clips.len());
    let notes = |s: &Session| -> usize {
        s.clips
            .iter()
            .map(|c| match &c.data {
                ClipData::Midi { notes, .. } => notes.len(),
                _ => 0,
            })
            .sum()
    };
    assert_eq!(notes(&back), notes(&demo));
    for (a, b) in demo.tracks.iter().zip(&back.tracks) {
        assert_eq!(
            (a.name.as_str(), a.kind.as_str()),
            (b.name.as_str(), b.kind.as_str())
        );
        if a.kind == "midi" {
            let instrument = |s: &Session, id: &str| {
                s.strips
                    .get(id)
                    .cloned()
                    .unwrap_or_default()
                    .instrument_name()
            };
            assert_eq!(
                instrument(&demo, &a.id),
                instrument(&back, &b.id),
                "{}",
                a.name
            );
        }
    }
}
