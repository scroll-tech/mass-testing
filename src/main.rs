#![allow(dead_code)]
use anyhow::{anyhow, bail, Result};
use ethers_providers::{Http, Provider};
use log4rs::{
    append::{
        console::{ConsoleAppender, Target},
        file::FileAppender,
    },
    config::{Appender, Config, Logger, Root},
};
use prover::{
    inner::Prover,
    types::WitnessBlock,
    utils::{read_env_var, short_git_version, GIT_VERSION},
    zkevm::circuit::{
        block_traces_to_witness_block, calculate_row_usage_of_witness_block, SuperCircuit,
    },
    BlockTrace, ChunkProof,
};
use reqwest::{ClientBuilder, Url};
use serde::Deserialize;
use std::{backtrace, env, panic, process::ExitCode, str::FromStr};

// build common config from enviroment
fn common_log() -> Result<Config> {
    dotenv::dotenv().ok();
    // TODO: cannot support complicated `RUST_LOG` for now.
    let log_level = read_env_var("RUST_LOG", "INFO".to_string());
    let log_level = log::LevelFilter::from_str(&log_level).unwrap_or(log::LevelFilter::Info);

    let stdoutput = ConsoleAppender::builder().target(Target::Stdout).build();

    let config = Config::builder()
        .appenders([Appender::builder().build("std", Box::new(stdoutput))])
        .build(Root::builder().appender("std").build(log_level))?;

    Ok(config)
}

// build config for circuit-debug
fn debug_log(output_dir: &str) -> Result<Config> {
    use std::path::Path;
    let app_output = ConsoleAppender::builder().target(Target::Stdout).build();
    let log_file_path = Path::new(output_dir).join("runner.log");
    let log_file = FileAppender::builder().build(log_file_path).unwrap();
    let config = Config::builder()
        .appenders([
            Appender::builder().build("log-file", Box::new(log_file)),
            Appender::builder().build("std", Box::new(app_output)),
        ])
        .logger(
            Logger::builder()
                .appender("std")
                .additive(true)
                .build("testnet_runner", log::LevelFilter::Info),
        )
        .build(
            Root::builder()
                .appender("log-file")
                .build(log::LevelFilter::Debug),
        )?;

    Ok(config)
}

fn prepare_output_dir(base_dir: &str, sub_dir: &str) -> Result<String> {
    use std::{fs, io::ErrorKind, path::Path};
    let path = Path::new(base_dir).join(sub_dir);
    fs::create_dir(path.as_path()).or_else(|e| match e.kind() {
        ErrorKind::AlreadyExists => fs::metadata(path.as_path()).map(|_| ()),
        _ => Err(e),
    })?;
    Ok(path
        .to_str()
        .ok_or_else(|| anyhow!("invalid output path"))?
        .into())
}

fn record_chunk_traces(chunk_dir: &str, traces: &[BlockTrace]) -> Result<()> {
    use flate2::{write::GzEncoder, Compression};
    use std::{fs::File, path::Path};
    use tar::{Builder, Header};

    let trace_file_path = Path::new(chunk_dir).join("traces.tar.gz");
    let tarfile = File::create(trace_file_path)?;
    let enc = GzEncoder::new(tarfile, Compression::default());
    let mut tar = Builder::new(enc);

    for (i, trace) in traces.iter().enumerate() {
        let trace_str = serde_json::to_string(&trace)?;

        let mut header = Header::new_gnu();
        header.set_path(trace.header.number.map_or_else(
            || format!("unknown_block_{i}.json"),
            |blkn| format!("{blkn}.json"),
        ))?;
        header.set_mode(0o644);
        header.set_size(trace_str.len() as u64);
        header.set_cksum();
        tar.append(&header, trace_str.as_bytes())?;
    }

    Ok(())
}

fn make_failure(dir: &str, data: &str) -> Result<()> {
    use std::{fs, path::Path};
    fs::write(Path::new(dir).join("failure"), data)?;
    Ok(())
}

const EXIT_NO_MORE_TASK: u8 = 9;
const EXIT_TEMP_NETWORK_ISSUE: u8 = 11;
const EXIT_FAILED_ENV: u8 = 13;
const EXIT_FAILED_ENV_WITH_TASK: u8 = 17;

fn task_core<R: Default + Sized>(
    output_dir: &str,
    log_handle: &log4rs::Handle,
    runner: impl FnOnce() -> Result<R> + panic::UnwindSafe,
) -> Result<R> {
    use std::sync::{Arc, RwLock};
    let panic_message = Arc::new(RwLock::new(String::from(
        "unknown error, message not recorded",
    )));

    // prepare for running test phase
    let out_err = panic_message.clone();
    panic::set_hook(Box::new(move |panic_info| {
        let err_msg = format!(
            "{} \nbacktrace: {}",
            panic_info,
            backtrace::Backtrace::capture(),
        );

        log::error!("{}", err_msg);
        match out_err.write() {
            Ok(mut error_str) => {
                *error_str = err_msg;
            }
            Err(e) => {
                log::error!("fail to write error message: {:?}", e);
            }
        }
    }));

    let handling_ret = panic::catch_unwind(runner);

    let _ = panic::take_hook();
    // this can not be handled gracefully
    log_handle.set_config(common_log().unwrap());

    handling_ret
        .map_err(|_| {
            // panic and we catch it in `panic_message`
            let err_msg = panic_message
                .read()
                .map(|reader| reader.clone())
                .unwrap_or(String::from("can not access panic_message"));
            anyhow!("testnet panic: {}", err_msg)
        })
        .and_then(|r| r)
        .or_else(|e| {
            log::info!("written encountered chunk error in {output_dir}");
            let err_content = e.to_string();
            if err_content.contains("no gpu device") {
                bail!("the error is a false positive result caused by hardware failure");
            } else if let Err(e) = make_failure(output_dir, &err_content) {
                log::error!("backup handling err: \n{err_content}");
                bail!("can not output error data into {output_dir}: {e:?}");
            }
            Ok(Default::default())
        })
}

#[tokio::main]
async fn main() -> ExitCode {
    let log_handle = log4rs::init_config(common_log().unwrap()).unwrap();

    let setting = Setting::new();

    let cli = ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .expect("run-testnet: failed to initialize ethers Provider from system configuration");
    let provider = Provider::new(Http::new_with_client(
        Url::from_str(&setting.l2geth_api_url)
            .expect("run-testnet: url failed to initialize ethers Provider"),
        cli,
    ));

    log::info!("git version {}", GIT_VERSION);
    log::info!("short git version {}", short_git_version());
    log::info!("settings: {setting:?}");

    log::info!("relay-alpha testnet runner: begin");

    let (id, chunks) = match get_chunks_info(&setting).await {
        Ok(r) => r,
        Err(e) => {
            log::error!("run-testnet: failed to request API err {e:?}");
            return ExitCode::from(EXIT_FAILED_ENV);
        }
    };

    // Save batch-id to do further batch-proving if it's a batch.
    let batch_id = if let BatchOrChunkId::Batch(batch_id) = id {
        Some(batch_id)
    } else {
        None
    };

    let mut chunks_task_complete = true;
    let mut expected_failed_exit_code = EXIT_FAILED_ENV;
    let mut chunk_proofs = vec![];
    match chunks {
        None => {
            log::info!("run-testnet: finished to prove at {id}");
            return ExitCode::from(EXIT_NO_MORE_TASK);
        }
        Some(chunks) => {
            // TODO: restart from last chunk?
            for chunk in chunks {
                let chunk_id = chunk.index;
                log::info!("chunk {:?}", chunk);

                // fetch traces
                let mut block_traces: Vec<BlockTrace> = vec![];
                for block_id in chunk.start_block_number..=chunk.end_block_number {
                    log::info!("run-testnet: requesting trace of block {block_id}");

                    match provider
                        .request(
                            "scroll_getBlockTraceByNumberOrHash",
                            [format!("{block_id:#x}")],
                        )
                        .await
                    {
                        Ok(trace) => {
                            block_traces.push(trace);
                        }
                        Err(e) => {
                            log::error!("obtain trace from block provider fail: {e:?}");
                            break;
                        }
                    }
                }

                if block_traces.len()
                    < (chunk.end_block_number - chunk.start_block_number + 1) as usize
                {
                    expected_failed_exit_code = EXIT_TEMP_NETWORK_ISSUE;
                    chunks_task_complete = false;
                    break;
                }

                // start chunk-level testing
                let chunk_dir =
                    prepare_output_dir(&setting.data_output_dir, &format!("chunk_{chunk_id}"))
                        .and_then(|chunk_dir| {
                            record_chunk_traces(&chunk_dir, &block_traces)?;
                            Ok(chunk_dir)
                        })
                        .and_then(|chunk_dir| {
                            log::info!("chunk {} has been recorded to {}", chunk_id, chunk_dir);
                            log_handle.set_config(debug_log(&chunk_dir)?);
                            Ok(chunk_dir)
                        });
                if let Err(e) = chunk_dir {
                    log::error!(
                        "can not prepare output enviroment for chunk {}: {:?}",
                        chunk_id,
                        e
                    );
                    chunks_task_complete = false;
                    break;
                }
                let chunk_dir = chunk_dir.expect("ok ensured");
                let spec_tasks = setting.spec_tasks.clone();

                let handling_ret = task_core(&chunk_dir, &log_handle, move || {
                    let witness_block = build_block(&block_traces, chunk_id)
                        .map_err(|e| anyhow::anyhow!("testnet: building block failed {e:?}"))?;

                    // mock
                    let mut chunk_proof = None;
                    if spec_tasks.iter().any(|str| str.as_str() == "mock") {
                        Prover::<SuperCircuit>::mock_prove_witness_block(&witness_block)
                            .map_err(|e| {
                                anyhow::anyhow!("testnet: failed to mock prove chunk {chunk_id} inside {id}:\n{e:?}")
                            })?;
                    }

                    // prove
                    if spec_tasks.iter().any(|str| str.as_str() == "prove") {
                        // Set ENV SCROLL_PROVER_ASSETS_DIR for asset files and
                        // SCROLL_PROVER_PARAMS_DIR for param files.
                        let proof =
                            prover::test::chunk_prove(&format!("chunk_{chunk_id}"), &witness_block);

                        // Save chunk-proof only for further batch-proving.
                        if batch_id.is_some() {
                            chunk_proof = Some(proof);
                        }
                    }

                    // panic: for test
                    if spec_tasks.iter().any(|str| str.as_str() == "panic") {
                        panic!("test panic");
                    }

                    Ok(chunk_proof)
                });

                match handling_ret {
                    Ok(chunk_proof) => {
                        chunk_proofs.push(chunk_proof);
                    }
                    Err(e) => {
                        log::info!("stop this node since error: {e}");
                        chunks_task_complete = false;
                        break;
                    }
                }
                log::info!("chunk {} has been handled", chunk_id);
            }
        }
    }

    // batch prove
    let task_complete = if let Some(batch_id) = batch_id {
        let spec_tasks = setting.spec_tasks.clone();
        prepare_output_dir(&setting.data_output_dir, &format!("batch_{batch_id}"))
            .and_then(|dir| {
                log::info!("batch {} has been recorded to {}", batch_id, dir);
                log_handle.set_config(debug_log(&dir)?);
                task_core(&dir, &log_handle, move || {
                    // check prove vector
                    if !chunks_task_complete || chunk_proofs.iter().any(Option::is_none) {
                        bail!("some chunk proof is not success, fail aggregation");
                    }

                    let chunk_hashes_proofs = chunk_proofs
                        .into_iter()
                        .map(Option::unwrap)
                        .map(|proof| (proof.chunk_hash.unwrap(), proof))
                        .collect();

                    if spec_tasks.iter().any(|str| str.as_str() == "agg") {
                        // note: would panic if any error raised
                        prover::test::batch_prove(&format!("{id}"), chunk_hashes_proofs);
                    }
                    Ok(())
                })
            })
            .map_err(|e| {
                log::error!("stop node since failure in aggregation: {}", e);
                e
            })
            .is_ok()
    } else {
        chunks_task_complete
    };

    if let Err(e) = notify_chunks_complete(&setting, *id.as_ref(), task_complete).await {
        log::error!("can not deliver complete notify to coordinator: {e:?}");
        return ExitCode::from(EXIT_FAILED_ENV_WITH_TASK);
    }

    if task_complete {
        log::info!("relay-alpha testnet runner: complete");
        ExitCode::from(0)
    } else {
        ExitCode::from(expected_failed_exit_code)
    }
}

fn build_block(block_traces: &[BlockTrace], chunk_id: i64) -> anyhow::Result<WitnessBlock> {
    let gas_total: u64 = block_traces
        .iter()
        .map(|b| b.header.gas_used.as_u64())
        .sum();
    let witness_block = block_traces_to_witness_block(block_traces)?;
    let rows = calculate_row_usage_of_witness_block(&witness_block)?;
    log::info!(
        "rows of chunk {chunk_id}(block range {:?} to {:?}):",
        block_traces.first().and_then(|b| b.header.number),
        block_traces.last().and_then(|b| b.header.number),
    );
    for r in &rows {
        log::info!("rows of {}: {}", r.name, r.row_num_real);
    }
    let row_num = rows.iter().map(|x| x.row_num_real).max().unwrap();
    log::info!(
        "final rows of chunk {chunk_id}: row {}, gas {}, gas/row {:.2}",
        row_num,
        gas_total,
        gas_total as f64 / row_num as f64
    );
    Ok(witness_block)
}

#[derive(Debug, Clone, Copy)]
enum BatchOrChunkId {
    Batch(i64),
    Chunk(i64),
}

impl std::fmt::Display for BatchOrChunkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Batch(i) => write!(f, "batch-{i}"),
            Self::Chunk(i) => write!(f, "chunk-{i}"),
        }
    }
}

impl AsRef<i64> for BatchOrChunkId {
    fn as_ref(&self) -> &i64 {
        match self {
            Self::Batch(i) => i,
            Self::Chunk(i) => i,
        }
    }
}

/// Request chunk info from cordinator
async fn get_chunks_info(setting: &Setting) -> Result<(BatchOrChunkId, Option<Vec<ChunkInfo>>)> {
    let url = Url::parse(&setting.chunks_url)?;

    let resp: String = reqwest::get(url).await?.text().await?;
    log::debug!("resp is {resp}");
    let resp: RollupscanResponse = serde_json::from_str(&resp)?;

    // we compatible with both chunk/batch resp. For chunk resp,
    // batch_index is set to 'null' in json
    if let Some(batch_index) = resp.batch_index {
        log::info!(
            "handling batch {}, chunk size {}",
            batch_index,
            resp.chunks.as_ref().unwrap().len()
        );
        Ok((BatchOrChunkId::Batch(batch_index), resp.chunks))
    } else if let Some(block_infos) = resp.blocks {
        if block_infos.is_empty() {
            Ok((BatchOrChunkId::Chunk(-1), None))
        } else {
            let chunk_index = resp.chunk_index;
            log::info!("handling single chunk {chunk_index}",);
            // Fold block infos
            let total_tx_num = block_infos.iter().map(|blk| blk.tx_num).sum::<i64>();
            let start_block_number = block_infos
                .iter()
                .map(|blk| blk.number)
                .min()
                .expect("should has one");
            let end_block_number = block_infos
                .iter()
                .map(|blk| blk.number)
                .max()
                .expect("should has one");
            Ok((
                BatchOrChunkId::Chunk(chunk_index),
                Some(vec![ChunkInfo {
                    index: chunk_index,
                    total_tx_num,
                    start_block_number,
                    end_block_number,
                    created_at: Default::default(),
                    hash: Default::default(),
                }]),
            ))
        }
    } else {
        Ok((BatchOrChunkId::Batch(-1), None))
    }
}

async fn notify_chunks_complete(setting: &Setting, index: i64, completed: bool) -> Result<()> {
    let url = Url::parse_with_params(
        &setting.task_url,
        &[(if completed { "done" } else { "drop" }, index.to_string())],
    )?;

    let resp = reqwest::get(url).await?.text().await?;
    log::info!(
        "notify index {} {}, resp {}",
        index,
        if completed { "done" } else { "drop" },
        resp,
    );
    Ok(())
}

#[derive(Deserialize, Debug)]
struct RollupscanResponse {
    batch_index: Option<i64>,
    #[serde(default)]
    chunk_index: i64,
    chunks: Option<Vec<ChunkInfo>>,
    blocks: Option<Vec<BlockInfo>>,
}

#[derive(Deserialize, Debug)]
struct ChunkInfo {
    index: i64,
    created_at: String,
    total_tx_num: i64,
    hash: String,
    start_block_number: i64,
    end_block_number: i64,
}

#[derive(Deserialize, Debug)]
struct BlockInfo {
    number: i64,
    tx_num: i64,
    hash: String,
    block_timestamp: String,
}

#[derive(Debug, Default)]
struct Setting {
    chunks_url: String,
    task_url: String,
    l2geth_api_url: String,
    data_output_dir: String,
    spec_tasks: Vec<String>,
}

impl Setting {
    pub fn new() -> Self {
        let l2geth_api_url =
            env::var("L2GETH_API_URL").expect("run-testnet: Must set env L2GETH_API_URL");
        let coordinator_url = env::var("COORDINATOR_API_URL");
        let (chunks_url, task_url) = if let Ok(url_prefix) = coordinator_url {
            (
                Url::parse(&url_prefix)
                    .and_then(|url| url.join("chunks"))
                    .expect("run-testnet: Must be valid url for coordinator api"),
                Url::parse(&url_prefix)
                    .and_then(|url| url.join("tasks"))
                    .expect("run-testnet: Must be valid url for coordinator api"),
            )
        } else {
            (
                Url::parse(&env::var("CHUNKS_API_URL").expect(
                    "run-test: CHUNKS_API_URL must be set if COORDINATOR_API_URL is not set",
                ))
                .expect("run-testnet: Must be valid url for chunks api"),
                Url::parse(&env::var("TASKS_API_URL").expect(
                    "run-test: TASKS_API_URL must be set if COORDINATOR_API_URL is not set",
                ))
                .expect("run-testnet: Must be valid url for tasks api"),
            )
        };

        let data_output_dir = env::var("OUTPUT_DIR").unwrap_or("output".to_string());

        let spec_tasks_str = env::var("TESTNET_TASKS").unwrap_or_default();
        let spec_tasks = spec_tasks_str.split(',').map(String::from).collect();

        Self {
            l2geth_api_url,
            data_output_dir,
            chunks_url: chunks_url.as_str().into(),
            task_url: task_url.as_str().into(),
            spec_tasks,
        }
    }
}

// Return true if success, false otherwise.
fn batch_prove(
    batch_id: i64,
    output_dir: &str,
    log_handle: &log4rs::Handle,
    chunk_proofs: Vec<ChunkProof>,
) -> bool {
    let output_dir = prepare_output_dir(output_dir, &format!("batch_{batch_id}")).and_then(|dir| {
        log::info!("batch {} has been recorded to {}", batch_id, dir);
        log_handle.set_config(debug_log(&dir)?);
        Ok(dir)
    });
    if let Err(e) = output_dir {
        log::error!(
            "can not prepare output enviroment for batch {}: {:?}",
            batch_id,
            e
        );
        return false;
    }
    let output_dir = output_dir.expect("ok ensured");

    let panic_message = std::sync::Arc::new(std::sync::RwLock::new(String::from(
        "unknown error, message not recorded",
    )));

    let out_err = panic_message.clone();
    panic::set_hook(Box::new(move |panic_info| {
        let err_msg = format!(
            "{} \nbacktrace: {}",
            panic_info,
            backtrace::Backtrace::capture(),
        );

        match out_err.write() {
            Ok(mut error_str) => {
                *error_str = err_msg;
            }
            Err(e) => {
                log::error!(
                    "fail to write error message: {:?}\n backup testnet panic {}",
                    e,
                    err_msg
                );
            }
        }
    }));

    let chunk_hashes_proofs = chunk_proofs
        .into_iter()
        .map(|proof| (proof.chunk_hash.unwrap(), proof))
        .collect();
    let handling_ret = panic::catch_unwind(|| {
        prover::test::batch_prove(&format!("batch_{batch_id}"), chunk_hashes_proofs)
    });

    let _ = panic::take_hook();
    log_handle.set_config(common_log().unwrap());

    if let Err(e) = handling_ret.map_err(|_| {
        // panic and we catch it in `panic_message`
        let err_msg = panic_message
            .read()
            .map(|reader| reader.clone())
            .unwrap_or(String::from("can not access panic_message"));
        anyhow!("testnet panic: {}", err_msg)
    }) {
        log::info!("encounter some error in batch {batch_id}");
        let err_content = e.to_string();
        // some special handling to avoid false positive
        if err_content.contains("no gpu device") {
            log::error!("the error is a false positive result caused by hardware failure, stop this node on {}: {:?}", batch_id, e);
            return false;
        }

        if let Err(e) = make_failure(&output_dir, &err_content) {
            log::error!("can not output error data for batch {}: {:?}", batch_id, e);
            return false;
        }
    }
    log::info!("batch {} has been handled", batch_id);

    true
}

#[cfg(test)]
mod test {
    use super::*;
    use prover::{test::chunk_prove, utils::get_block_trace_from_file};

    // Long running prove on GPU
    #[ignore]
    #[test]
    fn test_chunk_and_batch_proving() {
        let output_dir = read_env_var("OUTPUT_DIR", "output".to_string());
        let trace_path = read_env_var("TRACE_PATH", "trace.json".to_string());

        let log_handle = log4rs::init_config(common_log().unwrap()).unwrap();

        let block_trace = get_block_trace_from_file(trace_path);
        let witness_block = block_traces_to_witness_block(&[block_trace]).unwrap();

        let chunk_proof = chunk_prove("chunk_1", &witness_block);
        assert!(batch_prove(1, &output_dir, &log_handle, vec![chunk_proof]));
    }
}
