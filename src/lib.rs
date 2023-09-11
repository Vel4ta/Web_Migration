use std::fs::{File, create_dir};
use chrono::Utc;
use std::io::{BufReader, BufWriter, Write, BufRead};
use std::path::Path;
use bytes::Bytes;
use reqwest::{Client, Method, RequestBuilder};
use std::time::Duration;
use error_chain::error_chain;
use tokio::runtime::{Runtime, Builder};
// use image::io::Reader as ImageReader;
use std::collections::HashSet;

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
            _ => String::from("bad path"),
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
    fn build(mut paths: Vec<Paths>) -> Self {
        let (mut d, mut t, mut b, mut r) = (Paths::Bad, Paths::Bad, Paths::Bad, Paths::Bad);
        while let Some(path) = paths.pop() {
            (d, t, b, r) = match path {
                Paths::Departments(_) => (path, t, b, r),
                Paths::Targets(_) => (d, path, b, r),
                Paths::BaseUrl(_) => (d, t, path, r),
                Paths::Reports(_) => (d, t, b, path),
                _ => (d, t, b, r),
            }
        }

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
            |v: char| v == ';' || v == ',',
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
            extension: match extension {
                Some(ext) => join_by(ext, String::new(), "/"),
                _ => String::new()
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
            |v| v == '/',
            Target::build
        )
    }
}

struct Department {
    base: Paths,
    path: Target,
    today: Today,
}

impl Department {
    fn build(path: Target, today: Today, base: Paths) -> Self {
        Self {
            base: base,
            path: path,
            today: today,
        }
    }

    fn location(&self) -> String {
        self.base.make_path(self.path.to_path())
    }

    fn storage_location_today(&self) -> String {
        self.location() + &self.today.date.get() + &"/"
    }
    
    fn storage_location_now(&self) -> String {
        self.storage_location_today() + &self.today.time.get() + &"/"
    }

    fn file_location(&self) -> String {
        self.storage_location_now() + &self.path.to_store()
    }

    fn create_path(&self) -> Result<()> {
        let loc = self.location();
        if !Path::new(&loc).is_dir() {
            create_dir(loc)?;
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

    fn destroy(self) -> (Paths, Target, Today) {
        (self.base, self.path, self.today)
    }
}

struct Report {
    info: Department,
    data: Vec<String>,
}

impl Report {
    fn new(info: Department) -> Self {
        Self {
            info: info,
            data: Vec::new(),
        }
    }

    fn add(&mut self, report: String) -> () {
        self.data.push(report)
    }

    fn build(self) -> Result<String> {
        match self.info.create_path() {
            Ok(_) => Ok(
                self.info.store(
                    Bytes::from(
                        self.data
                            .iter()
                            .fold(String::new(), |acc, item| {
                                acc + item + ",\n"
                            })
                            .as_bytes()
                            .to_owned()
                    )
                )
            ),
            Err(e) => Err(e),
        }
    }
}

pub struct Manager;

impl Manager {
    pub fn run(base_path: &str) -> Result<String> {
        if !Path::new(&base_path).is_dir() {
            return Err(Error::from("Invalid base path"))
        }

        if let Some(c) = ConfigPath::prep_paths(base_path) {
            let paths = ConfigPath::build(c);

            let isolated_targets = Targets::prep_targets(&paths.targets);

            let targets = Targets::build(isolated_targets);
        
            let report = pursue_targets(targets, paths)?;

            report.build()
        } else {
            Err(Error::from("Missing config file. Make sure config.txt exists in config/. Make sure it has appropriate content."))
        }
    }
}

fn read_file(path: String) -> Result<BufReader<File>>{
    let file = File::open(path)?;
    Ok(BufReader::new(file))
}

fn rest<T>(data: &[T]) -> &[T] {
    match data {
        [_, b @ ..] => b,
        _ => &[]
    }
}

fn bytes_match(a: &[u8], b: &[u8], count: usize) -> usize {
    match a {
        [c, d @ ..] if !b.is_empty() && b.first().unwrap() == c => bytes_match(d, rest(b), count + 1),
        _ => count
    }
}

fn proper_scan_bytes(data: Bytes, content_marker: &str, end_content_marker: &str, tag_marker: &str, attribute_marker: &str, pattern_marker: &str, deliminator_marker: &str) -> HashSet<Vec<u8>> {
    let l = data.len();
    let mut i = 0;
    let mut j = 0;
    let mut k;
    let mut scan = HashSet::new();
    // where to start scanning
    let content_marker = content_marker.as_bytes();
    let cl = content_marker.len();
    // where to stop scanning
    let end_content_marker = end_content_marker.as_bytes();
    let el = end_content_marker.len();
    // tag to find. syntax: <tag
    let tag_marker = tag_marker.as_bytes();
    let tl = tag_marker.len();
    // attribute in tag
    let attribute_marker = attribute_marker.as_bytes();
    let al = attribute_marker.len();
    // pattern to search for within value of attribute
    let pattern_marker = pattern_marker.as_bytes();
    let pl = pattern_marker.len();
    // deliminates the end of an attribute e.g. " or the space character
    let deliminator_marker = deliminator_marker.as_bytes();
    let dl = deliminator_marker.len();

    // look for where to start scanning
    while i + cl < l {
        j = bytes_match(&data[i..i+cl], &content_marker, j);
        i += j;
        if j == cl {
            break;
        }
        j = 0;
        i += 1;
    }
    // start scanning for tag marker. Also check for end content marker
    while i + tl < l {
        j = bytes_match(&data[i..i+tl], &tag_marker, 0);
        if j == tl && i + j + al < l {
            i += j;
            // searching for attribute
            loop {
                j = bytes_match(&data[i..i+al], &attribute_marker, 0);
                i += j + 1;
                if i + al >= l || j == al {
                    break;
                }
            }
            if j == al && i + j + 1 + pl < l {
                // {attribute}=" therefore i += j + 1
                // doesn't work for {attribute}= e.g. width=300
                i += 1;
                j = i;

                // searching for end of attribute
                loop {
                    k = bytes_match(&data[j..j+dl], &deliminator_marker, 0);
                    j += k + 1;
                    if j + dl > l || k == dl {
                        break;
                    }
                }

                // this was easier than finding pattern before deliminator
                // but this forces us to rescan :(

                // check the pattern we're looking for is in attribute value
                let mut temp = 0;
                while temp < j - 2 - i && j - 2 - (i + temp) > pl && bytes_match(&data[i+temp..j-2], &pattern_marker, 0) != pl {
                    temp += 1;
                }

                // store value of atrribute into scan
                if j <= l && j - 2 - (i + temp) > pl {
                    scan.insert(Vec::from(&data[i..j-2]));
                }
            } else {
                j = i;
            }
            i = j;
        } else if i + el < l {
            // checking if we can stop scanning
            k = bytes_match(&data[i..i+el], &end_content_marker, 0);
            if k == el {
                break;
            }
            // trying to save us from rechecking same bytes
            i += j.max(k) + 1;
        } else {
            break;
        }
    }
    scan
}

// temporary solution
async fn download_files(scan: HashSet<Vec<u8>>, path: String) -> Result<()> {
    let mut base_path = String::from("T:/Web_Migration/files/");
    for part in path.split("/") {
        base_path += part;
        if !Path::new(&base_path).is_dir() {
            create_dir(&base_path)?;
        }
        base_path += "/";    
    }
    let path = base_path;

    for target in scan.iter() {
        let url = match target.first() {
            Some(b) if b == &b'/' => String::from("https://www.csun.edu") + String::from_utf8_lossy(target).as_ref(),
            Some(_) => String::from_utf8_lossy(target).as_ref().to_string(),
            _ => continue
        };

        let response = reqwest::get(url).await?;
        let fname = response
            .url()
            .path_segments()
            .and_then(|segments| segments.last())
            .and_then(|name| if name.is_empty() { None } else { name.split("?").next() })
            .and_then(|name| Some(name.replace("%20", " ")))
            .unwrap_or(String::from("tmp.bin"));

        println!("file to download: '{}'", fname);
        let fname = path.clone() + &fname;
        let content = response.bytes().await?;
        write_file(content, fname)?;
    }
    Ok(())
}

fn write_file(data: Bytes, path: String) -> Result<()> {
    let f = File::create(path)?;
    let mut f = BufWriter::new(f);
    f.write_all(&data)?;
    f.flush()?;
    Ok(())
}

fn a_client_and_runtime() -> Result<(Client, Runtime)> {
    let c = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?;
    let r = Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()?;
    Ok((c, r))
}

fn pursue_targets(mut targets: Targets, paths: ConfigPath) -> Result<Report> {
    match a_client_and_runtime() {
        Ok((client, rt)) => {
            let (mut dept, mut today) = (paths.departments, Today::build());

            let mut report = Report::new(
                Department::build(
                    Target::build(
                        &[String::from("reports")]
                    ),
                    Today::build(),
                    paths.reports,
                )
            );

            let mut count = 0;
            while let Some(target) = targets.pop() {
                let d = Department::build(target, today, dept);

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
                if count%3 == 0 {
                    count -= count;
                    std::thread::sleep(Duration::from_millis(1000));
                }

                match d.create_path() {
                    Ok(_) => match rt.block_on(handle) {
                        Ok(Some(content)) => {

                            // temporary solution
                            let copy = content.clone();
                            let scan = proper_scan_bytes(
                                copy,
                                "id=\"content\"",
                                "class=\"layout-csun--footer\"",
                                "<a ",
                                "href",
                                "/sites/default/files/",
                                "\""
                            );

                            let file_handle = rt.spawn(download_files(scan, d.path.to_url()));
                            if let Err(e) = rt.block_on(file_handle) {
                                println!("{e}");
                            }

                            let scan = proper_scan_bytes(
                                content.clone(),
                                "id=\"content\"",
                                "class=\"layout-csun--footer\"",
                                "<img ",
                                "src",
                                "/sites/default/files/",
                                "\""
                            );

                            let file_handle = rt.spawn(download_files(scan, d.path.to_url()));
                            if let Err(e) = rt.block_on(file_handle) {
                                println!("{e}");
                            }


                            report.add(d.store(content));
                        },
                        Ok(None) => println!("{}", d.path.to_url()),
                        Err(e) => println!("{e}"),
                    },
                    Err(e) => println!("{e}"),
                };

                (dept, _, today) = d.destroy();
            }
            Ok(report)
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
    if let [a, b @ ..] = s {
        join_by(b, acc + a + sep, sep)
    } else {
        join(s, acc)
    }
}

fn prep_data<T, F1, F2>(file_path: &str, f1: F1, f2: F2) -> Option<Vec<T>> 
where
    F1: Fn(char) -> bool,
    F2: Fn(&[String]) -> T {
    match read_file(String::from(file_path)) {
        Ok(buf) => Some(buf.lines()
            .fold(Vec::new(), |mut acc, item| {
                match item {
                    Ok(y) => {
                        let (mut tot, cur) = y.chars()
                            .fold(
                                (Vec::new(), String::new()),
                                |(mut tot, mut cur), item| match item {
                                    v if f1(v) => {
                                        tot.push(cur);
                                        (tot, String::new())
                                    },
                                    v if v != ' ' => {
                                        cur.push(item);
                                        (tot, cur)
                                    },
                                    _ => (tot, cur),
                                }
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

// fn prep_data_generic<T: Copy, F1, F2>(file_path: &str, f1: F1, f2: F2, acc1: T) -> Option<Vec<T>>
// where
//     F1: Fn((Vec<T>, T), char) -> (Vec<T>, T),
//     F2: Fn(&[T]) -> T {
//     match read_file(String::from(file_path)) {
//         Ok(buf) => Some(buf.lines()
//             .fold(Vec::new(), |mut acc2, item| {
//                 match item {
//                     Ok(y) => {
//                         let (mut tot, cur) = y
//                             .chars()
//                             .fold((Vec::new(), acc1), &f1);

//                         tot.push(cur);
//                         acc2.push(f2(&tot[..]));
//                     },
//                     Err(e) => println!("{e}"),
//                 };
//                 acc2
//             })
//         ),
//         Err(e) => {
//             println!("{e}");
//             None
//         },
//     }
// }