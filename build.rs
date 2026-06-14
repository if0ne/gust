use std::process::Command;

fn main() {
    // Allow skipping frontend build for faster backend iteration.
    // Also skip if npm is unavailable — graceful degradation.
    if std::env::var("SKIP_WEB_BUILD").is_ok() {
        println!("cargo:warning=Skipping web build (SKIP_WEB_BUILD is set)");
        ensure_dist_placeholder();
        return;
    }

    let web_dir = std::path::Path::new("web");
    if !web_dir.exists() {
        ensure_dist_placeholder();
        return;
    }

    // Detect npm. Fall back to placeholder rather than panicking.
    let npm = find_npm();
    let Some(npm) = npm else {
        println!("cargo:warning=npm not found — skipping web build, UI placeholder will be served");
        ensure_dist_placeholder();
        return;
    };

    println!("cargo:rerun-if-changed=web/src");
    println!("cargo:rerun-if-changed=web/package.json");
    println!("cargo:rerun-if-changed=web/vite.config.ts");
    println!("cargo:rerun-if-changed=web/index.html");

    let node_modules = web_dir.join("node_modules");
    if !node_modules.exists() {
        // Use `npm install` (not `npm ci`) so no package-lock.json is required.
        let status = Command::new(&npm)
            .args(["install"])
            .current_dir(web_dir)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(_) => {
                println!("cargo:warning=npm install failed — skipping web build");
                ensure_dist_placeholder();
                return;
            }
            Err(e) => {
                println!("cargo:warning=npm install error ({e}) — skipping web build");
                ensure_dist_placeholder();
                return;
            }
        }
    }

    let status = Command::new(&npm)
        .args(["run", "build"])
        .current_dir(web_dir)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            println!("cargo:warning=npm run build failed — serving placeholder UI");
            ensure_dist_placeholder();
        }
        Err(e) => {
            println!("cargo:warning=npm run build error ({e}) — serving placeholder UI");
            ensure_dist_placeholder();
        }
    }
}

fn find_npm() -> Option<String> {
    // On Windows, npm is often npm.cmd.
    for candidate in &["npm.cmd", "npm"] {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return Some((*candidate).to_owned());
        }
    }
    None
}

fn ensure_dist_placeholder() {
    let dist = std::path::Path::new("web/dist");
    if !dist.exists() {
        std::fs::create_dir_all(dist).unwrap();
    }
    let index = dist.join("index.html");
    if !index.exists() {
        std::fs::write(
            &index,
            "<html><body><h1>UI not built — install Node.js and run: cd web &amp;&amp; npm ci &amp;&amp; npm run build</h1></body></html>",
        )
        .unwrap();
    }
}
