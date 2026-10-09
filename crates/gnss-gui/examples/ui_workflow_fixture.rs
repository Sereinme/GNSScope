//! Generate an explicitly disposable, persisted UI test library; never reuse a directory.
use gnss_gui::{sample_pvt_view_model, GuiProcessingSettings, RunLibrary, RunStatus};
use std::{error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("pass a new absolute directory")?,
    );
    if !directory.is_absolute() {
        return Err("fixture directory must be absolute".into());
    }
    let duration: u64 = std::env::args()
        .nth(2)
        .map(|arg| arg.parse())
        .transpose()?
        .unwrap_or(120);
    if !(1..=1200).contains(&duration) {
        return Err("long source duration must be 1..=1200 seconds".into());
    }
    // create_dir refuses an existing target: no overwriting a prior run or user data.
    fs::create_dir(&directory)?;
    let source = directory.join("synthetic-zero-iq.dat");
    // 250 ms of 4 Msps Int16 IQ; no GNSS signal.
    fs::write(&source, vec![0_u8; 4_000_000])?;
    // Sparse zero IQ gives native queue/cancel checks enough time without allocating
    // a large in-memory buffer or changing the short immutable display fixtures.
    let long_source = directory.join(format!("synthetic-zero-iq-{duration}s.dat"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&long_source)?
        .set_len(duration * 4_000_000 * 4)?;
    let database = directory.join("runs.sqlite3");
    let library = RunLibrary::open(&database)?;
    library.set_preference("validation_fixture", "gnsscope-ui-workflow-v1")?;
    library.set_preference("theme", "dark")?;
    for index in 0..101 {
        let path = directory.join(format!("fixture-{index:03}.dat"));
        fs::hard_link(&source, &path)?;
        let settings = GuiProcessingSettings {
            iq_file_path: Some(path),
            ..Default::default()
        };
        let run = library.create_run(&settings)?;
        let status = match index % 10 {
            8 => RunStatus::Failed,
            9 => RunStatus::Cancelled,
            _ => RunStatus::Completed,
        };
        // A labelled display fixture, not a positioning result from the zero IQ file.
        if status == RunStatus::Completed {
            library.append_epoch(run.id, 0, &sample_pvt_view_model(), &[])?;
        }
        library.update_progress(
            run.id,
            status,
            1_000_000,
            1_000_000,
            usize::from(status == RunStatus::Completed),
            "Synthetic UI validation fixture",
        )?;
    }
    println!("Disposable fixture: {}", directory.display());
    println!("GNSSCOPE_TEST_LIBRARY={}", database.display());
    println!("Do not set GNSSCOPE_PREVIEW; it deliberately disables database access.");
    Ok(())
}
