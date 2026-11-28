use s3_studio_core::{accounts::AccountStore, folders, transfers::Engine};
use std::{sync::Arc, time::Duration};

pub fn run(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() || args == ["--help"] {
        println!("s3-studio sync list\ns3-studio sync plan <job>\ns3-studio sync run <job>");
        return Ok(());
    }
    let dir = super::backend::data_directory()?;
    let jobs = folders::jobs(&dir).map_err(anyhow::Error::msg)?;
    if args == ["list"] {
        for name in jobs.keys() {
            println!("{name}");
        }
        return Ok(());
    }
    anyhow::ensure!(
        args.len() == 2 && matches!(args[0].as_str(), "plan" | "run"),
        "s3-studio sync list | plan <job> | run <job>"
    );
    let job = jobs
        .get(&args[1])
        .ok_or_else(|| anyhow::anyhow!("{}: {}", crate::i18n::t("找不到同步任务"), args[1]))?;
    let store =
        AccountStore::open(dir.join("accounts.json")).map_err(|e| anyhow::anyhow!("{e}"))?;
    let db = tempfile::tempdir()?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let engine = Engine::open(db.path().join("transfers.json"), Arc::new(|_| {}))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let result = folders::sync(&dir, &engine, &store, job, args[0] == "run").await;
        let mut errors = Vec::new();
        match result {
            Ok(result) => println!("{}", serde_json::to_string_pretty(&result)?),
            Err(e) => errors.push(e),
        }
        loop {
            let list = engine.list().await;
            if list
                .iter()
                .all(|t| matches!(t.status.as_str(), "done" | "error" | "cancelled" | "paused"))
            {
                for t in list.into_iter().filter(|t| t.status != "done") {
                    errors.push(format!("{}: {}", t.key, t.error.unwrap_or(t.status)));
                    if let Err(e) =
                        s3_studio_core::transfer_commands::cancel_transfer(&engine, &store, t.id)
                            .await
                    {
                        errors.push(e);
                    }
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        anyhow::ensure!(errors.is_empty(), "{}", errors.join("\n"));
        Ok(())
    })
}
