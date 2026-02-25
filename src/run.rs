use crate::agent::create_agent;
use crate::config::AppConfig;
use crate::env::create_environment;
use crate::error::{Error, Result};
use crate::output;
use crate::store::Store;
use crate::template::functions::RenderContext;
use crate::template::render;

/// Run a single job by alias (one-shot execution).
pub async fn run_job(app: &AppConfig, alias: &str) -> Result<String> {
    let (_, job_config) = app
        .jobs
        .iter()
        .find(|(key, _)| key == alias)
        .ok_or_else(|| Error::Config(format!("no job found with alias '{alias}'")))?;

    let prompt_template = job_config
        .job
        .as_ref()
        .and_then(|j| j.prompt.as_deref())
        .ok_or_else(|| Error::Config(format!("job '{alias}' has no prompt configured")))?;

    let env_wrapper = create_environment(job_config.environment.as_ref())?;
    env_wrapper.ensure_ready()?;

    let agent = create_agent(&job_config.agent)?;

    let db_path = app.data_dir.join("vatic.db");
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Store(format!("cannot create data directory: {e}")))?;
    }
    let store = Store::open(&db_path)?;

    let mut ctx = RenderContext::new(app.dictionary.clone());
    ctx.memories = store.get_memories(alias, 100)?;

    let rendered_prompt = render(prompt_template, &ctx).await?;
    let system_prompt = job_config.agent.prompt.as_deref();

    let result = agent
        .run(&rendered_prompt, system_prompt, env_wrapper.as_ref())
        .await?;

    // If there's a history prompt, summarize the result before storing it
    let result_to_store = if let Some(history) = &job_config.history {
        let summary_prompt = format!("{}\n\n{}", history.prompt, result);
        match agent.run(&summary_prompt, None, env_wrapper.as_ref()).await {
            Ok(summary) => summary,
            Err(e) => {
                tracing::warn!("history summarization failed, storing raw result: {}", e);
                result.clone()
            }
        }
    } else {
        result.clone()
    };

    store.store_run(alias, &result_to_store)?;

    for output_section in &job_config.outputs {
        // Render the output's message template if it has one
        let rendered_message = if let Some(msg_template) = &output_section.message {
            let mut output_ctx = ctx.clone();
            output_ctx.result = Some(result.clone());
            Some(render(msg_template, &output_ctx).await?)
        } else {
            None
        };

        output::dispatch(output_section, &result, rendered_message.as_deref()).await?;
    }

    Ok(result)
}
