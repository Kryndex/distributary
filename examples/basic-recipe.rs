extern crate distributary;

mod backend;

use std::{thread, time};
use distributary::{Blender, Recipe};
use backend::Backend;

fn load_recipe() -> Backend {
    // inline recipe definition
    let sql = "# base tables
               CREATE TABLE Article (aid int, title varchar(255), \
                                     url text, PRIMARY KEY(aid));
               CREATE TABLE Vote (aid int, uid int);

               # read queries
               VoteCount: SELECT Vote.aid, COUNT(uid) AS votes \
                            FROM Vote GROUP BY Vote.aid;
               ArticleWithVoteCount: SELECT Article.aid, title, url, VoteCount.votes AS votes \
                            FROM Article, VoteCount \
                            WHERE Article.aid = VoteCount.aid AND Article.aid = ?;";

    // set up Soup via recipe
    let mut soup = Blender::new();
    soup.log_with(distributary::logger_pls());

    let recipe = soup.migrate(|mig| {
        // install recipe
        let mut recipe = Recipe::from_str(&sql, None).unwrap();
        recipe.activate(mig, false).unwrap();
        // return brings up new graph for processing
        recipe
    });

    Backend::new(soup, recipe)
}

fn main() {
    let mut backend = load_recipe();

    println!("Soup graph:\n{}", backend.soup);

    println!("Writing...");
    let aid = 1;
    let title = "test title";
    let url = "http://pdos.csail.mit.edu";
    let uid = 42;
    backend
        .put("Article", &[aid.into(), title.into(), url.into()])
        .unwrap();
    backend.put("Vote", &[aid.into(), uid.into()]).unwrap();

    println!("Finished writing! Let's wait for things to propagate...");
    thread::sleep(time::Duration::from_millis(1000));

    println!("Reading...");
    println!("{:#?}", backend.get("ArticleWithVoteCount", aid))
}
