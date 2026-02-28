use depplay_backend::analyze::analyze;

#[tokio::main]
async fn main() {
    let logs = r#"
Cloning repository...
Docker build started
Error: Could not find a production build in the '.next' directory.
Try running 'next build' before 'next start'.
"#;

    match analyze(logs).await {
        Ok(result) => {
            println!("Summary:\n{}\n", result.summary);

            println!("Issues:");
            for issue in result.issues {
                println!("- {}", issue);
            }

            println!("\nSuggestions:");
            for suggestion in result.suggestions {
                println!("- {}", suggestion);
            }
        }
        Err(e) => eprintln!("Error: {e}"),
    }
}
