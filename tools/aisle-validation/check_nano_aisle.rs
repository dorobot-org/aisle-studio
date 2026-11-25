//! Exercise Studio's production backend using an AISLE/Nexus Nano recording.
use dorobot_flex::api::{files::FileBackend, Backend, Intent, Tag};
use dorobot_flex::data::{video_decoder::VideoDecoder, LeRobotDataset};
use std::{fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::args().nth(1).ok_or("dataset path required")?);
    let ds = LeRobotDataset::open(&root)?;
    assert_eq!(ds.info.robot_type, "adora_nano");
    assert!(ds.info.total_frames > 0);
    assert_eq!(ds.episodes.len(), 3);
    let total_frames = ds.info.total_frames;
    let mut backend = FileBackend::with_open(&root, None);
    assert_eq!(backend.library().datasets.len(), 1);
    assert_eq!(backend.playback().state_names.len(), 17);
    assert_eq!(backend.playback().action_names.len(), 17);
    assert!(backend.playback().robot_urdf.is_some());
    let mut checked_video_frames = 0;
    for episode in 0..3 {
        backend.dispatch(Intent::SelectEpisode(episode));
        let data = ds.load_episode(episode)?;
        let playback = backend.playback();
        assert_eq!(playback.selected, Some(episode));
        assert_eq!(playback.joint_frames.len(), data.frames.len());
        if root
            .join(format!("meta/scene/episode_{episode:06}.json"))
            .exists()
        {
            let scene = playback.scene.as_ref().expect("recorded scene loaded");
            assert_eq!(scene.objects.len(), 7);
            assert_eq!(scene.frames.len(), data.frames.len());
        }
        assert_eq!(playback.state_series.len(), 17);
        assert_eq!(playback.action_series.len(), 17);
        for (channels, names, is_action) in [
            (&playback.state_series, &playback.state_names, false),
            (&playback.action_series, &playback.action_names, true),
        ] {
            for (index, channel) in channels.iter().enumerate() {
                assert_eq!(channel.name, names[index]);
                assert_eq!(channel.points.len(), data.frames.len());
                for (point, frame) in channel.points.iter().zip(&data.frames) {
                    let samples = if is_action {
                        &frame.action
                    } else {
                        &frame.state
                    };
                    assert_eq!(point.1, samples[index] as f64);
                }
            }
        }
        assert_eq!(playback.stats.state_channels, 17);
        assert_eq!(playback.stats.action_channels, 17);
        assert!(playback.stats.drift_frames.abs() < 1e-6);
        for (pose, source) in playback.joint_frames.iter().zip(&data.frames) {
            // Nano radians must reach the URDF unchanged, unlike SO100 normalization.
            assert_eq!(pose, &source.state);
        }
        let expected_cameras = ds
            .info
            .features
            .iter()
            .filter(|(_, f)| f.dtype == "video")
            .count();
        assert_eq!(data.video_paths.len(), expected_cameras);
        for (key, path) in &data.video_paths {
            let mut video = VideoDecoder::open(path)?;
            let shape = &ds.info.features[key].shape;
            let n = data.frames.len() as u64;
            for frame_index in [0, n / 2, n - 1, n / 4, 0, n - 1] {
                let frame = video.get_frame(frame_index)?;
                assert_eq!(frame.frame_index, frame_index);
                assert_eq!(
                    (frame.width as usize, frame.height as usize),
                    (shape[1] as usize, shape[0] as usize)
                );
                assert_eq!(frame.data.len(), (shape[0] * shape[1] * 3) as usize);
                checked_video_frames += 1;
            }
        }
        backend.dispatch(Intent::Seek(1.5));
        assert!((backend.playback().current_time - 1.5).abs() < 1e-9);
        backend.dispatch(Intent::StepFrames(1));
        assert!((backend.playback().current_time - 1.6).abs() < 1e-9);
        backend.dispatch(Intent::TogglePlay);
        assert!(backend.playback().is_playing);
        backend.dispatch(Intent::TogglePlay);
        assert!(!backend.playback().is_playing);
    }
    let original = fs::read(root.join("data/chunk-000/episode_000000.parquet"))?;
    backend.dispatch(Intent::TagEpisode {
        episode: 0,
        tag: Some(Tag::Good),
    });
    backend.dispatch(Intent::DeleteEpisode(0));
    assert_eq!(backend.playback().episodes.len(), 2);
    backend.dispatch(Intent::UndoExclude);
    assert_eq!(backend.playback().episodes.len(), 3);
    assert_eq!(
        original,
        fs::read(root.join("data/chunk-000/episode_000000.parquet"))?
    );
    drop(backend);
    let mut reopened = FileBackend::with_open(&root, None);
    assert_eq!(reopened.playback().episodes[0].tag, Some(Tag::Good));
    reopened.dispatch(Intent::TagEpisode {
        episode: 0,
        tag: None,
    });
    assert_eq!(reopened.playback().episodes[0].tag, None);
    println!(
        "{}",
        serde_json::json!({
            "ok":true,"episodes":3,"frames":total_frames,"state_channels":17,"action_channels":17,
            "video_seek_checks":checked_video_frames,"pose_values_preserved":true,
            "transport_intents":true,"tag_restart":true,"exclude_undo_preserved_bytes":true,
            "plotted_channels":reopened.playback().state_series.len(),
            "interactive_window_tested":false
        })
    );
    Ok(())
}
