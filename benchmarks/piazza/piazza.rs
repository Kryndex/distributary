extern crate distributary;

#[macro_use]
extern crate clap;

use std::{thread, time};
use std::fs::{File, OpenOptions};
use std::io::Write;

use distributary::{Base, Blender, DataType, Filter, Join, JoinType, Mutator, NodeIndex};

pub struct Piazza {
    pub soup: Blender,
    user: NodeIndex,
    post: NodeIndex,
    class: NodeIndex,
    taking: NodeIndex,
}

enum Fanout {
    All,
    Few,
}

impl Piazza {
    // Create the base nodes for our Piazza application
    pub fn new() -> Self {
        let mut g = Blender::new();
        g.log_with(distributary::logger_pls());

        let (user, post, class, taking) = g.migrate(|mig| {
            // add a user account base table
            let user = mig.add_ingredient("user", &["uid", "username", "hash"], Base::default());

            // add a post base table
            let post = mig.add_ingredient(
                "post",
                &["pid", "cid", "author", "content"],
                Base::default().with_key(vec![1]),
            );

            // add a class base table
            let class = mig.add_ingredient("class", &["cid", "classname"], Base::default());

            // associations between users and classes
            let taking = mig.add_ingredient("taking", &["cid", "uid"], Base::default());

            (user, post, class, taking)
        });

        Piazza {
            soup: g,
            user: user,
            post: post,
            class: class,
            taking: taking,
        }
    }

    pub fn log_user(&mut self, uid: DataType) {
        use distributary::Operator;

        let post = self.post;
        let taking = self.taking;
        self.soup.migrate(|mig| {
            // classes user is taking
            let class_filter = Filter::new(taking, &[None, Some((Operator::Equal, uid.into()))]);

            let user_classes = mig.add_ingredient("class_filter", &["cid", "uid"], class_filter);
            // add visible posts to user
            // only posts from classes the user is taking should be visible
            use distributary::JoinSource::*;
            let j = Join::new(
                post,
                user_classes,
                JoinType::Inner,
                vec![L(0), B(1, 0), L(2), L(3)],
            );
            let visible_posts =
                mig.add_ingredient("visible_posts", &["pid", "cid", "author", "content"], j);

            // maintain visible_posts
            mig.maintain(visible_posts, 0);
        });
    }
}

fn populate_users(nusers: i64, mut users_putter: Mutator) {
    for i in 0..nusers {
        users_putter
            .put(vec![i.into(), "user".into(), "pass".into()])
            .unwrap();
    }
}

fn populate_classes(nclasses: i64, mut class_putter: Mutator) {
    for i in 0..nclasses {
        class_putter.put(vec![i.into(), i.into()]).unwrap();
    }
}

fn populate_taking(nclasses: i64, nusers: i64, mut taking_putter: Mutator, fanout: Fanout) {
    match fanout {
        Fanout::Few => for j in 0..nusers {
            for i in 0..10 {
                let cid = (j * 10 + i) % nclasses;
                taking_putter.put(vec![cid.into(), j.into()]).unwrap();
            }
        },
        Fanout::All => for j in 0..nusers {
            for i in 0..nclasses {
                taking_putter.put(vec![i.into(), j.into()]).unwrap();
            }
        },
    }
}

fn main() {
    use clap::{App, Arg};
    let args = App::new("piazza")
        .version("0.1")
        .about("Benchmarks Piazza-like application with some security policies.")
        .arg(
            Arg::with_name("nclasses")
                .short("c")
                .long("classes")
                .value_name("N")
                .default_value("100")
                .help("Number of classes to prepopulate the database with"),
        )
        .arg(
            Arg::with_name("nusers")
                .short("u")
                .long("users")
                .value_name("N")
                .default_value("100")
                .help("Number of users to prepopulate the database with"),
        )
        .arg(
            Arg::with_name("nposts")
                .short("p")
                .long("posts")
                .value_name("N")
                .default_value("10000")
                .help("Number of posts to prepopulate the database with"),
        )
        .arg(
            Arg::with_name("csv")
                .long("csv")
                .required(false)
                .help("Print output in CSV format."),
        )
        .arg(
            Arg::with_name("fanout")
                .long("fanout")
                .short("f")
                .possible_values(&["all", "few"])
                .takes_value(true)
                .default_value("all")
                .help("Size of the class fanout for each user"),
        )
        .arg(
            Arg::with_name("benchmark")
                .possible_values(&["write", "migration"])
                .takes_value(true)
                .required(true)
                .help("Benchmark configuration"),
        )
        .get_matches();


    println!("Creating app...");
    let mut app = Piazza::new();
    println!("Done with app creation.");

    let nusers = value_t_or_exit!(args, "nusers", i64);
    let nclasses = value_t_or_exit!(args, "nclasses", i64);
    let nposts = value_t_or_exit!(args, "nposts", i64);
    let benchmark = args.value_of("benchmark").unwrap();
    let fanout = args.value_of("fanout").unwrap();
    let csv = args.is_present("csv");

    let class_putter = app.soup.get_mutator(app.class);
    let user_putter = app.soup.get_mutator(app.user);
    let taking_putter = app.soup.get_mutator(app.taking);
    let mut post_putter = app.soup.get_mutator(app.post);

    println!("Seeding...",);
    populate_users(nusers, user_putter);
    populate_classes(nclasses, class_putter);
    match fanout.as_ref() {
        "all" => populate_taking(nclasses, nusers, taking_putter, Fanout::All),
        "few" => populate_taking(nclasses, nusers, taking_putter, Fanout::Few),
        _ => {
            unreachable!();
        }
    }

    if benchmark == "migration" {
        for pid in 0..nposts {
            post_putter
                .put(vec![
                    pid.into(),
                    (pid % nclasses).into(),
                    (pid % nusers).into(),
                    "post".into(),
                ])
                .unwrap();
        }
    }

    println!("Finished seeding! Sleeping...");
    thread::sleep(time::Duration::from_millis(100));


    let mut times = Vec::new();
    if csv {
        File::create("out.csv").unwrap();
    }

    println!("Starting benchmark...");
    for uid in 0..nusers {
        let start;
        let end;
        start = time::Instant::now();
        match benchmark.as_ref() {
            "migration" => {
                app.log_user(uid.into());

                end = time::Instant::now().duration_since(start);
            }
            "write" => {
                for i in 0..1000 {
                    post_putter
                        .put(vec![
                            i.into(),
                            (i % nclasses).into(),
                            (i % nusers).into(),
                            "post".into(),
                        ])
                        .unwrap();
                }
                end = time::Instant::now().duration_since(start);

                thread::sleep(time::Duration::from_millis(100));

                app.log_user(uid.into());
            }
            _ => {
                unreachable!();
            }
        };

        let time = (end.as_secs() as f64) + (end.subsec_nanos() as f64 / 1_000_000_000.0);

        times.push(time);

        if csv {
            let mut f = OpenOptions::new()
                .write(true)
                .append(true)
                .open("out.csv")
                .unwrap();
            writeln!(f, "{:?},{:?}", uid, time).unwrap();
        } else {
            println!("{:?}: {:?}", uid, time);
        }
    }

    println!("{:?} results ", benchmark);
    println!("avg: {:?}", avg(&times));
    println!("max: {:?}", max_duration(&times));
    println!("min: {:?}", min_duration(&times));

    println!("Done with benchmark.");
}

fn max_duration(stats: &Vec<f64>) -> f64 {
    stats.iter().fold(0f64, |acc, el| f64::max(acc, *el))
}

fn min_duration(stats: &Vec<f64>) -> f64 {
    stats.iter().fold(stats[0], |acc, el| f64::min(acc, *el))
}


fn avg(stats: &Vec<f64>) -> f64 {
    stats.iter().sum::<f64>() / stats.len() as f64
}
