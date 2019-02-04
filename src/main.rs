use aes::Aes256;
use base64;
use block_modes::block_padding::Pkcs7;
use block_modes::{BlockMode, Cbc};
use md5;
use reqwest::header::HeaderMap;
use reqwest::{self, Client};
use serde_derive::Deserialize;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::exit;
use std::str;

const ACCESS_TOKEN: &str = "1rj2vRtegS8Y60B3w3qNZm5T2Q0TN2NR";

const KEY: &[u8; 35] = b"k8B$B@0L8D$tDYHGmRg98sQ7!%GOEGOX27T";

// https://stackoverflow.com/a/31976060
const BLACKLISTED_CHARS: &str = "<>:\"/\\|?*";

type Aes256Cbc = Cbc<Aes256, Pkcs7>;

#[derive(Deserialize, Clone)]
struct Anime {
    title: String,
    slug: AnimeSlug,
}

#[derive(Deserialize, Clone)]
struct AnimeSlug {
    slug: String,
}

#[derive(Deserialize, Clone)]
struct Source {
    source: String,
    number: u32,
}

fn input(prompt: &str) -> String {
    print!("{}: ", prompt);

    io::stdout().flush().expect("failed to flush stdout");

    let mut response = String::new();

    io::stdin()
        .read_line(&mut response)
        .expect("failed to read line from stdin");

    response.trim().to_string()
}

fn get_anime(query: &str, list: &[Anime], is_first: bool) -> Option<Anime> {
    loop {
        let list = list
            .iter()
            .filter(|x| x.title.to_lowercase().contains(query))
            .collect::<Vec<&Anime>>();

        if list.is_empty() {
            println!("no results found!\n");

            return None;
        }

        for (i, a) in list.iter().enumerate() {
            println!("{}. {}", i + 1, a.title);
        }

        if !is_first {
            println!("{}. go back", list.len() + 1);
        }

        if is_first {
            println!("{}. exit", list.len() + 1);
        }

        let response = match input(&format!("make a selection (1-{})", list.len() + 1))
            .trim()
            .parse::<usize>()
        {
            Ok(x) => x,
            Err(_) => {
                continue;
            }
        };

        if response <= list.len() && response != 0 {
            return Some(list[response - 1].clone());
        } else if response == list.len() + 1 && !is_first {
            return None;
        } else if response == list.len() + 1 && is_first {
            exit(0);
        }
    }
}

fn get_queue(list: &[Anime]) -> Vec<Anime> {
    let mut queue = Vec::new();

    let mut first = None;

    while first.is_none() {
        first = get_anime(
            input("enter the name of the anime you would like to download").trim(),
            list,
            true,
        );
    }

    queue.push(first.unwrap());

    loop {
        println!("current queue:");

        for a in queue.clone() {
            println!("- {}", a.title);
        }

        println!();

        println!("1. download");
        println!("2. add something to the queue");
        println!("3. exit");

        let response = match input("make a selection (1-3)").trim().parse::<usize>() {
            Ok(x) => x,
            Err(_) => {
                continue;
            }
        };

        match response {
            1 => {
                break;
            }
            2 => {
                let anime = get_anime(
                    input("enter the name of the anime you would like to download").trim(),
                    list,
                    false,
                );

                if anime.is_some() {
                    queue.push(anime.unwrap());
                }
            }
            3 => {
                exit(0);
            }
            _ => {}
        }
    }

    queue
}

fn get_safe_directory_name(s: &str) -> String {
    let mut safe = String::new();

    for c in BLACKLISTED_CHARS.split("") {
        safe = s.replace(c, "");
    }

    safe
}

// https://gitlab.com/ao/plugin.video.twistmoe/blob/master/twist.py#L46
fn derive_key_and_iv(
    password: &[u8; 35],
    salt: &[u8],
    key_length: usize,
    iv_length: usize,
) -> (Vec<u8>, Vec<u8>) {
    let mut d = b"".to_vec();
    let mut d_i = b"".to_vec();

    while d.len() < key_length + iv_length {
        let mut s = Vec::new();

        s.extend(d_i);

        s.extend(password.iter());

        s.extend(salt);

        d_i = <[u8; 16]>::from(md5::compute(s)).to_vec();

        d.extend(d_i.clone());
    }

    (
        d[..key_length].to_vec(),
        d[key_length..key_length + iv_length].to_vec(),
    )
}

fn main() {
    if env::var("CI").is_ok() {
        exit(0);
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-access-token",
        ACCESS_TOKEN
            .parse()
            .expect("failed to convert String to HeaderValue"),
    );

    let client = Client::builder()
        .default_headers(headers)
        .build()
        .expect("failed to create Client");

    let anime_list = client
        .get("https://twist.moe/api/anime")
        .send()
        .expect("failed to make request")
        .json::<Vec<Anime>>()
        .expect("failed to serialize response");

    let queue = get_queue(&anime_list);

    for a in queue {
        let episodes = client
            .get(&format!(
                "https://twist.moe/api/anime/{}/sources",
                a.slug.slug
            ))
            .send()
            .expect("failed to make request")
            .json::<Vec<Source>>()
            .expect("failed to serialize response");

        if !Path::new(&format!("Anime/{}", a.title)).exists() {
            fs::create_dir_all(Path::new(&format!("Anime/{}", get_safe_directory_name(&a.title))))
                .expect("failed to create directory");
        }

        for e in episodes {
            println!("downloading episode {} of {}...", e.number, a.title);

            let mut source = base64::decode(&e.source).expect("failed to decode source");

            assert_eq!(source[0..8], b"Salted__"[..]);

            let kiv = derive_key_and_iv(KEY, &source[8..16], 32, 16);

            let cipher = Aes256Cbc::new_var(&kiv.0, &kiv.1).expect("failed to create cipher");

            let source = cipher
                .decrypt(&mut source[16..])
                .expect("failed to decrypt source");

            let mut response = reqwest::get(&format!(
                "https://twist.moe{}",
                str::from_utf8(source).expect("failed to convert bytes to str")
            ))
            .expect("failed to make request");

            let mut file = File::create(format!("Anime/{}/{}.mp4", get_safe_directory_name(&a.title), e.number))
                .expect("failed to create file");

            io::copy(&mut response, &mut file).expect("failed to copy response to file");
        }
    }
}
