//! 整个 build.rs 就做一件事情，把 schema.sql 应用到
//! target/sqlx_schema.db，然后给 sqlx 编译用。
use std::process; // For panic
use std::{env, fs, path::PathBuf};

// build.rs runs in a sync context, so we need a runtime to run sqlx async functions
use fuck_backslash::FuckBackslash;
use path_absolutize::Absolutize;
// We need the execute trait from sqlx prelude
use sqlx::Executor;
// Import sqlx types needed
use sqlx::{Connection, SqliteConnection}; /* Use Connection trait and specific
                                            * SqliteConnection */
use tokio::runtime::Runtime;

fn main() {
    // Run the async setup function using a tokio runtime
    let rt = Runtime::new().expect("Failed to create Tokio runtime for build script");
    if let Err(e) = rt.block_on(setup_schema_db()) {
        eprintln!("Error during build script database setup: {}", e);
        process::exit(1); // Signal build failure
    }
}

async fn setup_schema_db() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=schema.sql");
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let target_dir = manifest_dir.join("target");
    let db_filename = "sqlx_schema.db"; // Consistent name
    let db_path = target_dir.join(db_filename);
    let schema_path = manifest_dir.join("schema.sql");

    // Ensure target directory exists
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir)?;
        println!(
            "cargo:warning=Created target directory: {}",
            target_dir.display()
        );
    }

    // Construct the connection URL for creation (needs to allow creation)
    // Note: Using canonicalize here might fail if the file doesn't exist yet.
    // We build the URL based on the *intended* absolute path first.
    // For Windows, paths need careful handling, canonicalize helps later.
    let db_url_for_creation = format!("sqlite:{}?mode=rwc", db_path.display()); // mode=rwc (ReadWriteCreate)

    // Only create and setup if the database file doesn't exist
    if !db_path.exists() {
        println!(
            "cargo:warning=Schema DB file {} not found. Creating and initializing from {}.",
            db_path.display(),
            schema_path.display()
        );

        if !schema_path.exists() {
            // Use panic! in build scripts for fatal errors that should stop the build
            panic!("schema.sql not found at {}", schema_path.display());
        }

        // Read schema.sql
        let schema_sql = fs::read_to_string(&schema_path)?;

        // Connect using sqlx (this will create the file due to mode=rwc)
        let mut conn = match SqliteConnection::connect(&db_url_for_creation).await {
            Ok(c) => c,
            Err(e) => {
                panic!(
                    "Failed to connect to (create) SQLite database at '{}': {}",
                    db_url_for_creation,
                    e // Use the creation URL in error msg
                );
            }
        };

        // Execute the schema script
        match conn.execute(&*schema_sql).await {
            // Pass schema_sql as &str
            Ok(_) => {
                println!(
                    "cargo:warning=Successfully created and initialized schema DB: {}",
                    db_path.display()
                );
            }
            Err(e) => {
                // Attempt to clean up the partially created file on error
                println!(
                    "cargo:warning=Failed to execute schema SQL. Attempting to remove partially created DB file: {}",
                    db_path.display()
                 );
                let _ = fs::remove_file(&db_path); // Ignore error on removal
                panic!(
                    "Failed to execute schema SQL from {}: {}\nSQL:\n{}",
                    schema_path.display(),
                    e,
                    schema_sql
                );
            }
        };
        // Connection is closed when `conn` goes out of scope
    } else {
        println!(
            "cargo:warning=Schema DB file {} already exists. Skipping creation.",
            db_path.display()
        );
    }

    // --- Set DATABASE_URL environment variable for sqlx macros ---
    // Now that the file is guaranteed to exist, get its canonical path.
    let absolute_db_path = match db_path.absolutize() {
        Ok(p) => p.into_owned().fuck_backslash(),
        Err(e) => panic!(
            "Failed to get canonical path for {}: {}",
            db_path.display(),
            e
        ),
    };
    let db_path_str = absolute_db_path
        .to_str()
        .ok_or("DB path is not valid UTF-8")?;

    // Construct the final DATABASE_URL for compile-time checks.
    // It does NOT need mode=rwc, just the path.
    // IMPORTANT: Use the `sqlite://` prefix for sqlx compile-time checks!
    let database_url_for_macros = format!("sqlite://{}", db_path_str);

    // Set the environment variable
    println!("cargo:rustc-env=DATABASE_URL={}", database_url_for_macros);
    println!(
        "cargo:warning=DATABASE_URL for compile-time checks set to: {}",
        database_url_for_macros
    );

    Ok(())
}
