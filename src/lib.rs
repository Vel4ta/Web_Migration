use std::fs::{File, create_dir};
use chrono::Utc;
use std::io::{BufReader, BufWriter, Write, BufRead};
use std::path::Path;
use bytes::Bytes;
use reqwest::{Client, Method, RequestBuilder};
use std::time::Duration;
use error_chain::error_chain;
use tokio::runtime::{Runtime, Builder};

error_chain! {
    foreign_links {
        Reqwest(reqwest::Error);
        Io(std::io::Error);
        Tokio(tokio::task::JoinError);
    }
}

enum Daily {
    Date(String),
    Time(i64),
}

impl Daily {
    fn get(&self) -> String {
        match self {
            Daily::Date(s) => s.to_string(),
            Daily::Time(t) => t.to_string(),
        }
    }
}

struct Today {
    date: Daily,
    time: Daily,
}

impl Today {
    fn build() -> Self {
        let now = Utc::now();
        Self {
            date: Daily::Date(now.date().to_string()),
            time: Daily::Time(now.timestamp()),
        }
    }
}

#[derive(Clone)]
enum Paths {
    Departments(String),
    Targets(String),
    BaseUrl(String),
    Reports(String),
    Bad,
}

impl Paths {
    fn from(path: &[String], base_path: &str) -> Self {
        if let [a, b @ ..] = path {
            match a.as_str() {
                "Departments" => Paths::Departments(join(b, String::from(base_path))),
                "Targets" => Paths::Targets(join(b, String::from(base_path))),
                "BaseUrl" => Paths::BaseUrl(join(b, String::new())),
                "Reports" => Paths::Reports(join(b, String::from(base_path))),
                _ => Paths::Bad,
            }
        } else {
            println!("bad config path");
            Paths::Bad
        }
    }

    fn get_path(&self) -> String {
        match self {
            Paths::Departments(p) |
            Paths::Reports(p) |
            Paths::Targets(p) |
            Paths::BaseUrl(p) => String::new() + p,
            Paths::Bad => String::from("bad path"),
        }
    }

    fn make_path(&self, path: String) -> String {
        self.get_path() + &path
    }
}

struct ConfigPath {
    departments: Paths,
    targets: Paths,
    base_url: Paths,
    reports: Paths,
}

impl ConfigPath {
    fn build(paths: Vec<Paths>) -> Self {
        let (d, t, b, r) = paths.iter()
            .fold(
                (Paths::Bad, Paths::Bad, Paths::Bad, Paths::Bad),
                |(d, t, b, r), path| {
                    match *path {
                        Paths::Departments(_) => (path.clone(), t, b, r),
                        Paths::Targets(_) => (d, path.clone(), b, r),
                        Paths::BaseUrl(_) => (d, t, path.clone(), r),
                        Paths::Reports(_) => (d, t, b, path.clone()),
                        _ => (d, t, b, r)
                    }
                }
            );
        Self {
            departments: d,
            targets: t,
            base_url: b,
            reports: r,
        }
    }

    fn prep_paths(base_path: &str) -> Option<Vec<Paths>> {
        prep_data(
            "./config/config.txt",
            |(mut tot, mut cur), item| match item {
                ';' | ',' => {
                    tot.push(cur);
                    (tot, String::new())
                },
                v if v != ' ' => {
                    cur.push(item);
                    (tot, cur)
                },
                _ => (tot, cur),
            },
            |p: &[String]| Paths::from(p, &base_path)
        )
    }
}

struct Target {
    base: String,
    extension: String,
}

impl Target {
    fn build(t: &[String]) -> Self {
        let (base, extension) = match t {
            [] => (String::new(), None),
            [a] => (String::from(a), None),
            [a, b @ ..] => (String::from(a), Some(b)),
        };

        Self {
            base: base,
            extension: if let Some(ext) = extension {
                join_by(ext, String::new(), "/")
            } else {
                String::new()
            },
        }
    }

    fn to_path(&self) -> String {
        String::new() + &self.base + &"/"
    }

    fn to_url(&self) -> String {
        self.to_path() + &self.extension
    }

    fn to_store(&self) -> String {
        self.extension
            .chars()
            .fold(String::new(), |mut acc, item| {
                if item == '/' {
                    acc.push('-')
                } else {
                    acc.push(item)
                }
                acc
            })
        + &".txt"
    }
}

struct Targets {
    targets: Vec<Target>,
}

impl Targets {
    fn build(prepped: Option<Vec<Target>>) -> Self {
        Self {
            targets: match prepped {
                Some(p) => p,
                _ => Vec::new(),
            },
        }
    }

    fn pop(&mut self) -> Option<Target> {
        self.targets.pop()
    }

    fn prep_targets(target_path: &Paths) -> Option<Vec<Target>> {
        prep_data(
            &target_path.get_path(),
            |(mut tot, mut cur), item| match item {
                '/' => {
                    tot.push(cur);
                    (tot, String::new())
                },
                v if v != ' ' => {
                    cur.push(item);
                    (tot, cur)
                },
                _ => (tot, cur),
            },
            Target::build
        )
    }
}

struct Department {
    base: Paths,
    path: Target,
    timestamp: String,
    date: String,
}

impl Department {
    fn build(path: Target, today: &Today, base: &Paths) -> Self {
        Self {
            base: base.clone(),
            path: path,
            timestamp: today.time.get(),
            date: today.date.get(),
        }
    }

    fn location(&self) -> String {
        self.base.make_path(self.path.to_path())
    }

    fn storage_location_today(&self) -> String {
        self.location() + &self.date + &"/"
    }
    
    fn storage_location_now(&self) -> String {
        self.storage_location_today() + &self.timestamp + &"/"
    }

    fn file_location(&self) -> String {
        self.storage_location_now() + &self.path.to_store()
    }

    fn create_path(&self) -> Result<()> {
        if !Path::new(&self.location()).is_dir() {
            create_dir(&self.location())?;
        }

        let date = self.storage_location_today();
        if !Path::new(&date).is_dir() {
            create_dir(date)?;
        }

        let time = self.storage_location_now();
        if !Path::new(&time).is_dir() {
            create_dir(time)?;
        }

        Ok(())
    }

    fn store(&self, data: Bytes) -> String {
        if let Err(e) = write_file(data, self.file_location()) {
            println!("{e}");
            String::from("No data for ") + &self.path.to_url()   
        } else {
            self.file_location()
        }
    }
}

pub struct Manager;

impl Manager {
    pub fn run(base_path: &str) -> Result<Vec<String>> {
        if Path::new(&base_path).is_dir() {
            match ConfigPath::prep_paths(base_path) {
                Some(c) => {
                    let paths = ConfigPath::build(c);

                    let isolated_targets = Targets::prep_targets(&paths.targets);

                    let targets = Targets::build(isolated_targets);
            
                    let report = pursue_targets(targets, paths)?;
            
                    Ok(report)
                },
                _ => {
                    Err(Error::from("bad config file"))
                },
            }
        } else {
            Err(Error::from("Invalid base path"))
        }
    }
}

fn read_file(path: String) -> Result<BufReader<File>>{
    let file = File::open(path)?;
    Ok(BufReader::new(file))
}

fn write_file(data: Bytes, path: String) -> Result<()> {
    let f = File::create(path)?;
    let mut f = BufWriter::new(f);
    f.write_all(&data)?;
    Ok(())
}

fn a_client_and_runtime() -> Result<(Client, Runtime)> {
    let c = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let r = Builder::new_multi_thread()
        // .worker_threads(1)
        .enable_all()
        .build()?;
    Ok((c, r))
}

fn pursue_targets(mut targets: Targets, paths: ConfigPath) -> Result<Vec<String>> {
    match a_client_and_runtime() {
        Ok((client, rt)) => {
            let today = Today::build();
            let mut results = Vec::<String>::new();
            let mut count = 0;
            while let Some(target) = targets.pop() {
                let d = Department::build(target, &today, &paths.departments);

                let handle = rt.spawn(
                    collect_content(
                        client.request(
                            Method::GET,
                            paths.base_url.make_path(
                                d.path.to_url()
                            )
                        )
                    )
                );
                
                count += 1;
                if count%10 == 0 {
                    std::thread::sleep(Duration::from_millis(3000));
                }

                match d.create_path() {
                    Ok(_) => match rt.block_on(handle) {
                        Ok(Some(content)) => results.push(
                            d.store(content)
                        ),
                        Ok(None) => println!("{}", d.path.to_url()),
                        Err(e) => println!("{e}"),
                    },
                    Err(e) => println!("{e}"),
                };
            }
            Ok(results)
        },
        Err(e) => Err(e),
    }
}

async fn collect_content(request: RequestBuilder) -> Option<Bytes> {
    match request.send().await {
        Ok(r) if r.status().is_success() => {
            match r.bytes().await {
                Ok(b) => Some(b),
                Err(e) => {
                    println!("{e}");
                    None
                },
            }
        },
        Ok(r) => {
            println!("{}", r.status());
            None
        },
        Err(e) => {
            println!("{e}");
            None
        },
    }
}

fn join(s: &[String], acc: String) -> String {
    match s {
        [] => acc,
        [a] => acc + a,
        [a, b @ ..] => join(b, acc + a),
    }
}

fn join_by(s: &[String], acc: String, sep: &str) -> String {
    match s {
        [] => acc,
        [a] => acc + a,
        [a, b @ ..] => join_by(b, acc + a + sep, sep),
    }
}

fn prep_data<
    T,
    F1: Fn((Vec<String>, String), char) -> (Vec<String>, String),
    F2: Fn(&[String]) -> T
    >(file_path: &str, f1: F1, f2: F2) -> Option<Vec<T>> {
    match read_file(String::from(file_path)) {
        Ok(buf) => Some(buf.lines()
            .fold(Vec::new(), |mut acc, item| {
                match item {
                    Ok(y) => {
                        let (mut tot, cur) = y.chars()
                            .fold(
                                (Vec::new(), String::new()),
                                &f1
                            );
                        tot.push(cur);
                        acc.push(f2(&tot[..]));
                    },
                    Err(e) => println!("{e}"),
                };
                acc
            })
        ),
        Err(e) => {
            println!("{e}");
            None
        },
    }
}
