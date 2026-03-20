use anyhow::{anyhow, Context, Result};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use std::{
    env,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::lookup_host, time::{sleep, timeout}};
use url::Url;

const DEFAULT_HOST: &str = "kotatsu.ruxel.net";
const DEFAULT_API_PORT: u16 = 8080;
const DEFAULT_QUIC_PORT: u16 = 4433;
const DEFAULT_SAMPLES: usize = 10;
const IO_TIMEOUT_SECS: u64 = 10;
const DATAGRAM_TIMEOUT_SECS: u64 = 3;

#[derive(Debug, Clone)]
struct Config {
    remote_host: String,
    remote_ip: Option<String>,
    api_port: u16,
    quic_port: u16,
    samples: usize,
    api_base_url: Option<String>,
    quic_override_url: Option<String>,
}

impl Config {
    fn api_base(&self) -> String {
        if let Some(url) = &self.api_base_url {
            return url.clone();
        }
        let target = self.remote_ip.as_deref().unwrap_or(&self.remote_host);
        format!("http://{target}:{}", self.api_port)
    }

    fn quic_override(&self) -> String {
        if let Some(url) = &self.quic_override_url {
            return url.clone();
        }
        let target = self.remote_ip.as_deref().unwrap_or(&self.remote_host);
        format!("quic://{target}:{}", self.quic_port)
    }
}

fn parse_u16(value: &str, flag: &str) -> Result<u16> {
    value
        .parse::<u16>()
        .with_context(|| format!("failed to parse {flag} as u16"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize> {
    value
        .parse::<usize>()
        .with_context(|| format!("failed to parse {flag} as usize"))
}

fn next_arg_value(args: &mut env::Args, flag: &str) -> Result<String> {
    args.next()
        .ok_or_else(|| anyhow!("missing value for {flag}"))
}

fn usage(program: &str) -> String {
    format!(
        "Usage: {program} [options]\n\
         \n\
         Measures post-connect QUIC datagram one-way latency over\n\
         client A -> server -> client B.\n\
         \n\
         Options:\n\
           --host <host>            Remote hostname (default: {DEFAULT_HOST})\n\
           --remote-ip <ip>         Use this IP for API/QUIC instead of resolving the host\n\
           --api-port <port>        HTTP API port (default: {DEFAULT_API_PORT})\n\
           --quic-port <port>       QUIC port (default: {DEFAULT_QUIC_PORT})\n\
           --samples <count>        Number of datagram samples (default: {DEFAULT_SAMPLES})\n\
           --api-base-url <url>     Override the full HTTP API base URL\n\
           --quic-url <url>         Override the full QUIC URL\n\
           -h, --help               Show this help\n\
         \n\
         Environment fallback:\n\
           REMOTE_HOST, REMOTE_IP, API_PORT, QUIC_PORT, RTT_SAMPLES,\n\
           API_BASE_URL, QUIC_OVERRIDE_URL\n"
    )
}

fn parse_config() -> Result<Config> {
    let mut config = Config {
        remote_host: env::var("REMOTE_HOST").unwrap_or_else(|_| DEFAULT_HOST.into()),
        remote_ip: env::var("REMOTE_IP").ok().filter(|value| !value.is_empty()),
        api_port: env::var("API_PORT")
            .ok()
            .as_deref()
            .map(|value| parse_u16(value, "API_PORT"))
            .transpose()?
            .unwrap_or(DEFAULT_API_PORT),
        quic_port: env::var("QUIC_PORT")
            .ok()
            .as_deref()
            .map(|value| parse_u16(value, "QUIC_PORT"))
            .transpose()?
            .unwrap_or(DEFAULT_QUIC_PORT),
        samples: env::var("RTT_SAMPLES")
            .ok()
            .as_deref()
            .map(|value| parse_usize(value, "RTT_SAMPLES"))
            .transpose()?
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_SAMPLES),
        api_base_url: env::var("API_BASE_URL").ok().filter(|value| !value.is_empty()),
        quic_override_url: env::var("QUIC_OVERRIDE_URL")
            .ok()
            .filter(|value| !value.is_empty()),
    };

    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "remote-rtt".into());

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => config.remote_host = next_arg_value(&mut args, "--host")?,
            "--remote-ip" => config.remote_ip = Some(next_arg_value(&mut args, "--remote-ip")?),
            "--api-port" => {
                let value = next_arg_value(&mut args, "--api-port")?;
                config.api_port = parse_u16(&value, "--api-port")?;
            }
            "--quic-port" => {
                let value = next_arg_value(&mut args, "--quic-port")?;
                config.quic_port = parse_u16(&value, "--quic-port")?;
            }
            "--samples" => {
                let value = next_arg_value(&mut args, "--samples")?;
                let parsed = parse_usize(&value, "--samples")?;
                if parsed == 0 {
                    return Err(anyhow!("--samples must be greater than 0"));
                }
                config.samples = parsed;
            }
            "--api-base-url" => {
                config.api_base_url = Some(next_arg_value(&mut args, "--api-base-url")?)
            }
            "--quic-url" => {
                config.quic_override_url = Some(next_arg_value(&mut args, "--quic-url")?)
            }
            "-h" | "--help" => {
                print!("{}", usage(&program));
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unknown argument: {arg}\n\n{}", usage(&program))),
        }
    }

    Ok(config)
}

#[derive(Debug, Deserialize)]
struct CreateMatchRes {
    match_id: String,
}

#[derive(Debug, Deserialize, Clone)]
struct JoinMatchRes {
    player_id: String,
    token: String,
    quic_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientReliable {
    Join { token: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ServerReliable {
    JoinOk {
        match_id: String,
        player_id: String,
        server_time_ms: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ClientDatagram {
    Pos {
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum ServerDatagram {
    Pos {
        player_id: String,
        seq: u64,
        x: f32,
        y: f32,
        vx: f32,
        vy: f32,
        server_time_ms: u64,
    },
}

#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

fn build_quic_client_config() -> Result<ClientConfig> {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    let client = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    Ok(ClientConfig::new(Arc::new(client)))
}

async fn write_json_line<T: Serialize>(send: &mut SendStream, msg: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec(msg)?;
    bytes.push(b'\n');
    send.write_all(&bytes).await?;
    Ok(())
}

async fn read_json_line<T: for<'de> Deserialize<'de>>(
    recv: &mut RecvStream,
    buf: &mut Vec<u8>,
) -> Result<T> {
    loop {
        if let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<u8>>();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            return Ok(serde_json::from_slice::<T>(line)?);
        }

        let chunk = timeout(
            Duration::from_secs(IO_TIMEOUT_SECS),
            recv.read_chunk(4096, true),
        )
        .await
        .context("timed out waiting for QUIC stream data")??;

        match chunk {
            Some(c) => buf.extend_from_slice(&c.bytes),
            None => return Err(anyhow!("stream closed")),
        }
    }
}

#[derive(Debug)]
struct ConnectedClient {
    player_id: String,
    _endpoint: Endpoint,
    conn: Connection,
    _reliable_send: SendStream,
    _reliable_recv: RecvStream,
}

#[derive(Debug)]
struct SampleStats {
    delivered: Vec<f64>,
    lost: usize,
}

fn summarize(name: &str, values: &[f64], lost: usize) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);

    let min = sorted.first().copied().unwrap_or_default();
    let max = sorted.last().copied().unwrap_or_default();
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let median = sorted[sorted.len() / 2];
    let p95_index = ((sorted.len() - 1) as f64 * 0.95).round() as usize;
    let p95 = sorted[p95_index];

    println!(
        "{name}: received={}/{} lost={} min={min:.2}ms mean={mean:.2}ms median={median:.2}ms p95={p95:.2}ms max={max:.2}ms",
        values.len(),
        values.len() + lost,
        lost
    );
}

async fn connect_and_join(join: &JoinMatchRes, quic_override_url: Option<&str>) -> Result<ConnectedClient> {
    let quic_url = quic_override_url.unwrap_or(&join.quic_url);
    let url = Url::parse(&quic_url.replace("quic://", "https://")).context("parse quic_url")?;
    let host = url.host_str().ok_or_else(|| anyhow!("quic_url host missing"))?;
    let port = url.port().ok_or_else(|| anyhow!("quic_url port missing"))?;
    let remote_addr = lookup_host((host, port))
        .await
        .context("resolve remote host")?
        .next()
        .ok_or_else(|| anyhow!("no remote addr resolved"))?;

    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(build_quic_client_config()?);

    let conn = timeout(
        Duration::from_secs(IO_TIMEOUT_SECS),
        endpoint.connect(remote_addr, host).context("connect call failed")?,
    )
    .await
    .context("timed out waiting for QUIC connect")??;

    let (mut send, mut recv) = timeout(Duration::from_secs(IO_TIMEOUT_SECS), conn.open_bi())
        .await
        .context("timed out opening reliable QUIC stream")??;

    write_json_line(
        &mut send,
        &ClientReliable::Join {
            token: join.token.clone(),
        },
    )
    .await?;

    let mut recv_buf = Vec::with_capacity(2048);
    match read_json_line::<ServerReliable>(&mut recv, &mut recv_buf).await? {
        ServerReliable::JoinOk { player_id, .. } if player_id == join.player_id => {}
        ServerReliable::JoinOk { player_id, .. } => {
            return Err(anyhow!(
                "joined with unexpected player_id: expected {} got {}",
                join.player_id,
                player_id
            ));
        }
        ServerReliable::Error { code, message } => {
            return Err(anyhow!("join failed: {code} {message}"));
        }
    }

    Ok(ConnectedClient {
        player_id: join.player_id.clone(),
        _endpoint: endpoint,
        conn,
        _reliable_send: send,
        _reliable_recv: recv,
    })
}

fn pos_payload(seq: u64) -> ClientDatagram {
    ClientDatagram::Pos {
        seq,
        x: seq as f32,
        y: 0.0,
        vx: 0.0,
        vy: 0.0,
    }
}

fn encoded_pos(seq: u64) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&pos_payload(seq))?)
}

async fn measure_datagram_one_way(
    initiator: &ConnectedClient,
    receiver: &ConnectedClient,
    samples: usize,
) -> Result<SampleStats> {
    let mut stats = SampleStats {
        delivered: Vec::with_capacity(samples),
        lost: 0,
    };

    for seq in 1..=samples as u64 {
        let payload = encoded_pos(seq)?;
        let started = Instant::now();
        initiator
            .conn
            .send_datagram(payload.into())
            .context("failed to send datagram probe")?;

        let received = timeout(Duration::from_secs(DATAGRAM_TIMEOUT_SECS), async {
            loop {
                let bytes = receiver.conn.read_datagram().await.context("read datagram failed")?;
                let parsed = serde_json::from_slice::<ServerDatagram>(&bytes);
                let ServerDatagram::Pos { player_id, seq: received_seq, .. } = match parsed {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if player_id == initiator.player_id && received_seq == seq {
                    return Ok::<f64, anyhow::Error>(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
        })
        .await;

        match received {
            Ok(Ok(one_way_ms)) => {
                println!("sample {seq}: datagram_one_way={one_way_ms:.2}ms");
                stats.delivered.push(one_way_ms);
            }
            Ok(Err(err)) => return Err(err),
            Err(_) => {
                println!("sample {seq}: datagram_one_way=timeout");
                stats.lost += 1;
            }
        }

        sleep(Duration::from_millis(80)).await;
    }

    Ok(stats)
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = parse_config()?;
    let api_base = config.api_base();
    let quic_override_url = Some(config.quic_override());
    let samples = config.samples;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(IO_TIMEOUT_SECS))
        .build()?;

    let create: CreateMatchRes = http
        .post(format!("{api_base}/v1/matches"))
        .json(&serde_json::json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let join_a: JoinMatchRes = http
        .post(format!("{api_base}/v1/matches/{}/join", create.match_id))
        .json(&serde_json::json!({"display_name": "rtt-a"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let join_b: JoinMatchRes = http
        .post(format!("{api_base}/v1/matches/{}/join", create.match_id))
        .json(&serde_json::json!({"display_name": "rtt-b"}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    println!("api base: {api_base}");
    println!("quic override: {}", quic_override_url.as_deref().unwrap_or(""));
    println!("remote host: {}", config.remote_host);
    if let Some(remote_ip) = &config.remote_ip {
        println!("remote ip override: {remote_ip}");
    }
    println!("samples: {samples}");
    println!("mode: datagram one-way latency from client A to client B via server");

    let initiator = connect_and_join(&join_a, quic_override_url.as_deref()).await?;
    let receiver = connect_and_join(&join_b, quic_override_url.as_deref()).await?;

    sleep(Duration::from_millis(150)).await;

    let stats = measure_datagram_one_way(&initiator, &receiver, samples).await?;

    initiator.conn.close(0u32.into(), b"done");
    receiver.conn.close(0u32.into(), b"done");

    println!("=== summary ===");
    if stats.delivered.is_empty() {
        println!(
            "datagram_one_way: received=0/{} lost={} no samples available",
            samples,
            stats.lost
        );
        return Ok(());
    }
    summarize("datagram_one_way", &stats.delivered, stats.lost);

    Ok(())
}
