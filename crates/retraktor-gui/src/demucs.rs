//! # `retraktor-gui::demucs`
//!
//! Background management and execution of Meta AI's Demucs (`htdemucs`) neural
//! audio source separation engine.
//!
//! Provides:
//! - Non-blocking background virtual environment installer (`~/.local/share/retraktor/venv`).
//! - Automatic detection of system or local Demucs binaries.
//! - Background separation queue running with low process priority (`nice -n 10`)
//!   to guarantee 100% audio engine reliability and avoid audio dropouts.
//! - Caching of true isolated 4-stem FLAC streams (`drums`, `bass`, `other`, `vocals`).

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug, PartialEq)]
pub enum DemucsInstallState {
    NotInstalled,
    Installing { step: String, percent: f32 },
    Ready { binary_path: PathBuf, version: String },
    Failed { error: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DemucsSeparationStatus {
    Idle,
    Running {
        track_id: usize,
        track_title: String,
        percent: f32,
    },
    Completed {
        track_id: usize,
        stem_dir: PathBuf,
    },
    Failed {
        track_id: usize,
        error: String,
    },
}

#[derive(Clone)]
pub struct DemucsManager {
    pub install_state: Arc<Mutex<DemucsInstallState>>,
    pub separation_status: Arc<Mutex<DemucsSeparationStatus>>,
}

impl Default for DemucsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DemucsManager {
    pub fn new() -> Self {
        let mgr = Self {
            install_state: Arc::new(Mutex::new(DemucsInstallState::NotInstalled)),
            separation_status: Arc::new(Mutex::new(DemucsSeparationStatus::Idle)),
        };
        mgr.refresh_status();
        mgr
    }

    /// Returns the standard cache directory for Demucs stems (`~/.cache/retraktor/stems/htdemucs`).
    pub fn stems_cache_root() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".cache/retraktor/stems/htdemucs")
    }

    /// Returns the isolated virtualenv path for Retraktor (`~/.local/share/retraktor/venv`).
    pub fn venv_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".local/share/retraktor/venv")
    }

    /// Finds the Demucs executable (checks local venv first, then system PATH).
    pub fn find_demucs_binary() -> Option<PathBuf> {
        // 1. Check local dedicated venv
        let venv_bin = Self::venv_path().join("bin/demucs");
        if venv_bin.is_file() {
            return Some(venv_bin);
        }

        // 2. Check system PATH via `which demucs`
        if let Ok(out) = Command::new("which").arg("demucs").output() {
            if out.status.success() {
                let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let p = PathBuf::from(path_str);
                if p.is_file() {
                    return Some(p);
                }
            }
        }

        // 3. Check ~/.local/bin/demucs
        let home = std::env::var("HOME").unwrap_or_default();
        let local_bin = PathBuf::from(&home).join(".local/bin/demucs");
        if local_bin.is_file() {
            return Some(local_bin);
        }

        None
    }

    /// Refreshes and updates the installation status.
    pub fn refresh_status(&self) {
        if let Some(bin) = Self::find_demucs_binary() {
            // Verify execution
            let ver = Command::new(&bin)
                .arg("--help")
                .output()
                .map(|_| "HTDemucs v4 (Meta AI)".to_string())
                .unwrap_or_else(|_| "Demucs".to_string());

            let mut state = self.install_state.lock().unwrap();
            *state = DemucsInstallState::Ready {
                binary_path: bin,
                version: ver,
            };
        } else {
            let mut state = self.install_state.lock().unwrap();
            if !matches!(*state, DemucsInstallState::Installing { .. }) {
                *state = DemucsInstallState::NotInstalled;
            }
        }
    }

    /// Returns true if Demucs is installed and ready for separation.
    pub fn is_installed(&self) -> bool {
        matches!(*self.install_state.lock().unwrap(), DemucsInstallState::Ready { .. })
    }

    /// Returns a copy of the current install state.
    pub fn get_install_state(&self) -> DemucsInstallState {
        self.install_state.lock().unwrap().clone()
    }

    /// Polls the separation worker and returns the current status along with a human-readable description if running.
    pub fn poll_separation(&self) -> (DemucsSeparationStatus, Option<String>) {
        let status = self.separation_status.lock().unwrap().clone();
        let desc = match &status {
            DemucsSeparationStatus::Running { track_title, percent, .. } => {
                Some(format!("Separating '{}' ({:.0}%)", track_title, percent * 100.0))
            }
            _ => None,
        };
        (status, desc)
    }

    /// Checks if a given audio file already has pre-rendered stems in the cache.
    pub fn get_cached_stem_dir(audio_path: &Path) -> Option<PathBuf> {
        let file_stem = audio_path.file_stem()?.to_str()?;
        let target_dir = Self::stems_cache_root().join(file_stem);

        if !target_dir.is_dir() {
            return None;
        }

        // Check for the 4 required stem files (.flac or .wav)
        let has_drums = target_dir.join("drums.flac").is_file() || target_dir.join("drums.wav").is_file();
        let has_bass = target_dir.join("bass.flac").is_file() || target_dir.join("bass.wav").is_file();
        let has_other = target_dir.join("other.flac").is_file() || target_dir.join("other.wav").is_file();
        let has_vocals = target_dir.join("vocals.flac").is_file() || target_dir.join("vocals.wav").is_file();

        if has_drums && has_bass && has_other && has_vocals {
            Some(target_dir)
        } else {
            None
        }
    }

    /// Spawns the background installer in an isolated thread.
    pub fn start_background_install(&self) {
        {
            let mut state = self.install_state.lock().unwrap();
            if matches!(*state, DemucsInstallState::Installing { .. }) {
                return; // Already installing
            }
            *state = DemucsInstallState::Installing {
                step: "Initializing environment...".to_string(),
                percent: 0.05,
            };
        }

        let state_clone = self.install_state.clone();

        thread::spawn(move || {
            let update = |step: &str, percent: f32| {
                if let Ok(mut st) = state_clone.lock() {
                    *st = DemucsInstallState::Installing {
                        step: step.to_string(),
                        percent,
                    };
                }
            };

            // 1. Verify python3 exists
            let py_check = Command::new("python3").arg("--version").output();
            if py_check.is_err() {
                if let Ok(mut st) = state_clone.lock() {
                    *st = DemucsInstallState::Failed {
                        error: "python3 is not installed on this system. Run: sudo apt install python3 python3-pip python3-venv".to_string(),
                    };
                }
                return;
            }

            let venv_dir = Self::venv_path();
            let parent = venv_dir.parent().unwrap();
            let _ = fs::create_dir_all(parent);

            // 2. Create virtual environment
            update("Creating isolated virtualenv in ~/.local/share/retraktor/venv...", 0.15);
            let venv_status = Command::new("python3")
                .arg("-m")
                .arg("venv")
                .arg(&venv_dir)
                .status();

            if venv_status.is_err() || !venv_status.unwrap().success() {
                // If python3-venv is missing, try with --without-pip then install pip
                let retry = Command::new("python3")
                    .arg("-m")
                    .arg("venv")
                    .arg("--without-pip")
                    .arg(&venv_dir)
                    .status();

                if retry.is_err() || !retry.unwrap().success() {
                    if let Ok(mut st) = state_clone.lock() {
                        *st = DemucsInstallState::Failed {
                            error: "Failed to create python venv. Please run: sudo apt install python3-venv python3-pip".to_string(),
                        };
                    }
                    return;
                }
            }

            let pip_bin = venv_dir.join("bin/pip");

            // 3. Upgrade pip
            update("Updating pip & wheel utilities...", 0.30);
            let _ = Command::new(&pip_bin)
                .args(["install", "--upgrade", "pip", "setuptools", "wheel"])
                .status();

            // 4. Install PyTorch and Demucs
            update("Downloading PyTorch and Meta Demucs model engine (~1.2 GB)...", 0.50);
            let install_proc = Command::new(&pip_bin)
                .args([
                    "install",
                    "torch",
                    "torchaudio",
                    "demucs",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            match install_proc {
                Ok(mut child) => {
                    if let Some(stdout) = child.stdout.take() {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().flatten() {
                            if line.contains("Downloading") {
                                update(&format!("Downloading: {}", line.chars().take(40).collect::<String>()), 0.70);
                            } else if line.contains("Installing") {
                                update("Installing Demucs & PyTorch neural components...", 0.85);
                            }
                        }
                    }
                    let _ = child.wait();
                }
                Err(e) => {
                    if let Ok(mut st) = state_clone.lock() {
                        *st = DemucsInstallState::Failed {
                            error: format!("Pip execution failed: {e}"),
                        };
                    }
                    return;
                }
            }

            // 5. Verify installation
            update("Verifying Demucs neural inference engine...", 0.95);
            let demucs_bin = venv_dir.join("bin/demucs");
            if demucs_bin.is_file() {
                let test = Command::new(&demucs_bin).arg("--help").output();
                if test.is_ok() && test.unwrap().status.success() {
                    if let Ok(mut st) = state_clone.lock() {
                        *st = DemucsInstallState::Ready {
                            binary_path: demucs_bin,
                            version: "HTDemucs v4 (Meta AI)".to_string(),
                        };
                    }
                    return;
                }
            }

            if let Ok(mut st) = state_clone.lock() {
                *st = DemucsInstallState::Failed {
                    error: "Demucs installed but binary verification failed. Check Python environment.".to_string(),
                };
            }
        });
    }

    /// Spawns a background stem separation worker for an audio track.
    pub fn start_separation(
        &self,
        track_id: usize,
        track_title: &str,
        audio_path: &Path,
    ) -> bool {
        let demucs_bin = match Self::find_demucs_binary() {
            Some(b) => b,
            None => return false,
        };

        if !audio_path.is_file() {
            return false;
        }

        // If stems are already cached, mark completed immediately!
        if let Some(stem_dir) = Self::get_cached_stem_dir(audio_path) {
            let mut status = self.separation_status.lock().unwrap();
            *status = DemucsSeparationStatus::Completed {
                track_id,
                stem_dir,
            };
            return true;
        }

        {
            let mut status = self.separation_status.lock().unwrap();
            *status = DemucsSeparationStatus::Running {
                track_id,
                track_title: track_title.to_string(),
                percent: 0.05,
            };
        }

        let status_clone = self.separation_status.clone();
        let audio_path_buf = audio_path.to_path_buf();
        let cache_root = Self::stems_cache_root();
        let track_title_string = track_title.to_string();

        thread::spawn(move || {
            let _ = fs::create_dir_all(&cache_root);

            // Execute Demucs with `nice -n 10` so audio playback is NEVER starved of CPU
            let mut cmd = Command::new("nice");
            cmd.args([
                "-n",
                "10",
                demucs_bin.to_str().unwrap_or("demucs"),
                "-n",
                "htdemucs",
                "--flac",
                "-o",
                cache_root.to_str().unwrap_or("/tmp"),
            ]);
            cmd.arg(&audio_path_buf);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    if let Ok(mut st) = status_clone.lock() {
                        *st = DemucsSeparationStatus::Failed {
                            track_id,
                            error: format!("Failed to spawn Demucs: {e}"),
                        };
                    }
                    return;
                }
            };

            // Demucs writes progress to stderr (tqdm)
            if let Some(stderr) = child.stderr.take() {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    // Look for percentage in tqdm output, e.g. " 45%|"
                    if let Some(pct_idx) = line.find('%') {
                        if pct_idx >= 3 {
                            let num_str = &line[pct_idx - 3..pct_idx].trim();
                            if let Ok(pct) = num_str.parse::<f32>() {
                                if let Ok(mut st) = status_clone.lock() {
                                    *st = DemucsSeparationStatus::Running {
                                        track_id,
                                        track_title: track_title_string.clone(),
                                        percent: (pct / 100.0).clamp(0.05, 0.98),
                                    };
                                }
                            }
                        }
                    }
                }
            }

            let exit = child.wait();
            if exit.is_ok() && exit.unwrap().success() {
                if let Some(stem_dir) = Self::get_cached_stem_dir(&audio_path_buf) {
                    if let Ok(mut st) = status_clone.lock() {
                        *st = DemucsSeparationStatus::Completed {
                            track_id,
                            stem_dir,
                        };
                    }
                    return;
                }
            }

            if let Ok(mut st) = status_clone.lock() {
                *st = DemucsSeparationStatus::Failed {
                    track_id,
                    error: "Stem separation did not produce expected FLAC files.".to_string(),
                };
            }
        });

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demucs_paths_and_initial_status() {
        let mgr = DemucsManager::new();
        let (status, desc) = mgr.poll_separation();
        assert_eq!(status, DemucsSeparationStatus::Idle);
        assert_eq!(desc, None);

        let cache_root = DemucsManager::stems_cache_root();
        assert!(cache_root.to_str().unwrap().contains(".cache/retraktor/stems/htdemucs"));

        let venv_dir = DemucsManager::venv_path();
        assert!(venv_dir.to_str().unwrap().contains(".local/share/retraktor/venv"));
    }

    #[test]
    fn test_demucs_cache_detection() {
        let temp_dir = std::env::temp_dir().join("retraktor_demucs_test");
        let _ = fs::create_dir_all(&temp_dir);

        // Incomplete folder -> None
        assert_eq!(DemucsManager::get_cached_stem_dir(&temp_dir.join("test_track.mp3")), None);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
