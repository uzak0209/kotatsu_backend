use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::{lookup_host, UdpSocket},
    time::{sleep, timeout},
};
use url::Url;

const DEFAULT_HOST: &str = "kotatsu.ruxel.net";
const DEFAULT_API_PORT: u16 = 8080;
const DEFAULT_UDP_PORT: u16 = 4433;
const DEFAULT_SAMPLES: usize = 10;
const IO_TIMEOUT_SECS: u64 = 10;
const DATAGRAM_TIMEOUT_SECS: u64 = 3;
const PKT_RELIABLE: u8 = 0x01;
const PKT_UNRELIABLE: u8 = 0x02;

#[derive(Debug, Clone)]
struct Config {
    remote_host: String,
    remote_ip: Option<String>,
    api_port: u16,
    udp_port: u16,
    samples: usize,
    api_base_url: Option<String>,
    udp_override_url: Option<String>,
}

impl Config {
    fn api_base(&self) -> String {
        if let Some(url) = &self.api_base_url {
            return url.clone();
        }
        let target = self.remote_ip.as_deref().unwrap_or(&self.remote_host);
        format!("http://{target}:{}", self.api_port)
    }

    fn udp_override(&self) -> String {
        if let Some(url) = &self.udp_override_url {
            return url.clone();
        }
        let target = self.remote_ip.as_deref().unwrap_or(&self.remote_host);
        format!("udp://{target}:{}", self.udp_port)
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
         Measures post-connect UDP datagram one-way latency over\n\
         client A -> server -> client B.\n\
         \n\
         Options:\n\
           --host <host>            Remote hostname (default: {DEFAULT_HOST})\n\
           --remote-ip <ip>         Use this IP for API/UDP instead of resolving the host\n\
           --api-port <port>        HTTP API port (default: {DEFAULT_API_PORT})\n\
           --udp-port <port>        UDP port (default: {DEFAULT_UDP_PORT})\n\
           --samples <count>        Number of datagram samples (default: {DEFAULT_SAMPLES})\n\
           --api-base-url <url>     Override the full HTTP API base URL\n\
           --udp-url <url>          Override the full UDP URL\n\
           -h, --help               Show this help\n\
         \n\
         Environment fallback:\n\
           REMOTE_HOST, REMOTE_IP, API_PORT, UDP_PORT, RTT_SAMPLES,\n\
           API_BASE_URL, UDP_OVERRIDE_URL\n"
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
        udp_port: env::var("UDP_PORT")
            .ok()
            .or_else(|| env::var("QUIC_PORT").ok())
            .as_deref()
            .map(|value| parse_u16(value, "UDP_PORT"))
            .transpose()?
            .unwrap_or(DEFAULT_UDP_PORT),
        samples: env::var("RTT_SAMPLES")
            .ok()
            .as_deref()
            .map(|value| parse_usize(value, "RTT_SAMPLES"))
            .transpose()?
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_SAMPLES),
        api_base_url: env::var("API_BASE_URL")
            .ok()
            .filter(|value| !value.is_empty()),
        udp_override_url: env::var("UDP_OVERRIDE_URL")
            .ok()
            .or_else(|| env::var("QUIC_OVERRIDE_URL").ok())
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
            "--udp-port" | "--quic-port" => {
                let value = next_arg_value(&mut args, arg.as_str())?;
                config.udp_port = parse_u16(&value, arg.as_str())?;
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
            "--udp-url" | "--quic-url" => {
                config.udp_override_url = Some(next_arg_value(&mut args, arg.as_str())?)
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
    udp_url: String,
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
    MatchStarted {
        match_id: String,
        started_at_unix: u64,
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
struct ConnectedClient {
    player_id: String,
    socket: Arc<UdpSocket>,
}

#[derive(Debug)]
struct SampleStats {
    delivered: Vec<f64>,
    lost: usize,
}

fn encode_packet<T: Serialize>(pkt_type: u8, msg: &T) -> Result<Vec<u8>> {
    let mut packet = vec![pkt_type];
    packet.extend_from_slice(&serde_json::to_vec(msg)?);
    Ok(packet)
}

fn parse_reliable(packet: &[u8]) -> Option<ServerReliable> {
    if packet.first().copied() != Some(PKT_RELIABLE) {
        return None;
    }
    serde_json::from_slice(&packet[1..]).ok()
}

fn parse_datagram(packet: &[u8]) -> Option<ServerDatagram> {
    if packet.first().copied() != Some(PKT_UNRELIABLE) {
        return None;
    }
    serde_json::from_slice(&packet[1..]).ok()
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

async fn connect_udp_socket(udp_url: &str) -> Result<Arc<UdpSocket>> {
    let url = Url::parse(udp_url).context("parse udp_url")?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("udp_url host missing"))?;
    let port = url.port().ok_or_else(|| anyhow!("udp_url port missing"))?;
    let remote_addr = lookup_host((host, port))
        .await
        .context("resolve remote host")?
        .next()
        .ok_or_else(|| anyhow!("no remote addr resolved"))?;

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(remote_addr).await?;
    Ok(Arc::new(socket))
}

async fn connect_and_join(
    join: &JoinMatchRes,
    udp_override_url: Option<&str>,
) -> Result<ConnectedClient> {
    let udp_url = udp_override_url.unwrap_or(&join.udp_url);
    let socket = connect_udp_socket(udp_url).await?;

    let join_packet = encode_packet(
        PKT_RELIABLE,
        &ClientReliable::Join {
            token: join.token.clone(),
        },
    )?;
    socket.send(&join_packet).await?;

    let mut buf = vec![0u8; 65536];
    loop {
        let len = timeout(Duration::from_secs(IO_TIMEOUT_SECS), socket.recv(&mut buf))
            .await
            .context("timed out waiting for UDP join response")??;
        let packet = &buf[..len];

        match parse_reliable(packet) {
            Some(ServerReliable::JoinOk { player_id, .. }) if player_id == join.player_id => {
                break;
            }
            Some(ServerReliable::JoinOk { player_id, .. }) => {
                return Err(anyhow!(
                    "joined with unexpected player_id: expected {} got {}",
                    join.player_id,
                    player_id
                ));
            }
            Some(ServerReliable::Error { code, message }) => {
                return Err(anyhow!("join failed: {code} {message}"));
            }
            Some(ServerReliable::MatchStarted { .. }) | None => {}
        }
    }

    Ok(ConnectedClient {
        player_id: join.player_id.clone(),
        socket,
    })
}

fn encoded_pos(seq: u64) -> Result<Vec<u8>> {
    encode_packet(
        PKT_UNRELIABLE,
        &ClientDatagram::Pos {
            seq,
            x: seq as f32,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
        },
    )
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
    let mut buf = vec![0u8; 65536];

    for seq in 1..=samples as u64 {
        let payload = encoded_pos(seq)?;
        let started = Instant::now();
        initiator
            .socket
            .send(&payload)
            .await
            .context("failed to send datagram probe")?;

        let received = timeout(Duration::from_secs(DATAGRAM_TIMEOUT_SECS), async {
            loop {
                let len = receiver
                    .socket
                    .recv(&mut buf)
                    .await
                    .context("read datagram failed")?;
                let packet = &buf[..len];
                let ServerDatagram::Pos {
                    player_id,
                    seq: received_seq,
                    ..
                } = match parse_datagram(packet) {
                    Some(v) => v,
                    None => continue,
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
    let config = parse_config()?;
    let api_base = config.api_base();
    let udp_override_url = Some(config.udp_override());
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
    println!(
        "udp override: {}",
        udp_override_url.as_deref().unwrap_or("")
    );
    println!("remote host: {}", config.remote_host);
    if let Some(remote_ip) = &config.remote_ip {
        println!("remote ip override: {remote_ip}");
    }
    println!("samples: {samples}");
    println!("mode: datagram one-way latency from client A to client B via server");

    let initiator = connect_and_join(&join_a, udp_override_url.as_deref()).await?;
    let receiver = connect_and_join(&join_b, udp_override_url.as_deref()).await?;

    sleep(Duration::from_millis(150)).await;

    let stats = measure_datagram_one_way(&initiator, &receiver, samples).await?;

    println!("=== summary ===");
    if stats.delivered.is_empty() {
        println!(
            "datagram_one_way: received=0/{} lost={} no samples available",
            samples, stats.lost
        );
        return Ok(());
    }
    summarize("datagram_one_way", &stats.delivered, stats.lost);

    Ok(())
}
