use std::{env, time::Duration};

use anyhow::Context;
use fake::{locales::EN, Fake};
use reqwest::{header::HeaderValue, Client};
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    let db = SqlitePoolOptions::new()
        .connect(&env::var("DATABASE_URL")?)
        .await
        .context("could not connect to database_url")?;
    sqlx::migrate!().run(&db).await?;
    let token = env::var("DISCORD_TOKEN").context("could not connect to discord")?;
    let author = env::var("DISCORD_AUTHOR")?
        .parse()
        .context("could not find author")?;
    let server = env::var("DISCORD_SERVER")?
        .parse()
        .context("no server id specified")?;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", HeaderValue::from_str(&token)?);
    headers.insert(
        "User-Agent",
        HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64; rv:109.0) Gecko/20100101 Firefox/115.0",
        ),
    );
    let client = reqwest::ClientBuilder::new()
        .default_headers(headers)
        .build()?;
    let mut sleep = 1000;

    let mut offset = 0;
    'main: loop {
        std::thread::sleep(Duration::from_millis(sleep));
        let Ok(messages) = get_messages(author, server, offset, &client).await else {
            sleep += 500;
            continue;
        };
        if messages.len() == 0 {
            println!("got 'em all");
            break;
        }
        for message in messages {
            let channel_id = message.channel_id;
            let message_id = message.id;
            let in_db = sqlx::query_scalar!(
                r#"select count(*) from message where channelId=$1 and messageId=$2"#,
                channel_id,
                message_id
            )
            .fetch_one(&db)
            .await?;
            if in_db > 0 {
                println!("hit the back: {:?}", message);
                // break 'main;
                continue;
            }
            println!("{}", message.content);
            let sentence = match get_article(&client).await {
                Ok(article) => format!(
                    "# {}\n\n> {}\n",
                    article.title,
                    article.extract.replace("==", "**")
                ),
                Err(err) => {
                    println!(
                        "failed to get article, falling back on lorem ipsum. {:?}",
                        err
                    );

                    fake::faker::lorem::raw::Sentence(EN, 8..16).fake()
                }
            };
            let Ok(_) = edit_message(&sentence, channel_id, message_id, &client).await else {
                println!("failed to edit, skipping...");
                continue;
            };
            sqlx::query!(
                "insert into message (channelId, messageId, content) values ($1, $2, $3)",
                message.channel_id,
                message.id,
                message.content
            )
            .execute(&db)
            .await?;
        }
        offset += 25;
        sleep = sleep.saturating_sub(100);
    }
    Ok(())
}

async fn get_messages(
    author: i64,
    server: i64,
    offset: i64,
    client: &Client,
) -> anyhow::Result<Vec<RealMsg>> {
    let messages = client.get(format!("https://discord.com/api/v9/guilds/{server}/messages/search?author_id={author}&include_nsfw=true&offset={offset}")).build()?;
    let messages = client.execute(messages).await?;
    let messages: MessageResponse = messages.json().await?;
    Ok(messages
        .messages
        .into_iter()
        .filter_map(|msg| msg.0.try_into().ok())
        .collect())
}

async fn edit_message(
    content: &str,
    channel: i64,
    message: i64,
    client: &Client,
) -> anyhow::Result<()> {
    let edit_msg = EditMsg {
        content: content.to_string(),
    };
    let patch = client
        .patch(format!(
            "https://discord.com/api/v9/channels/{channel}/messages/{message}"
        ))
        .json(&edit_msg)
        .build()?;
    client.execute(patch).await?;
    Ok(())
}

#[derive(serde::Serialize)]
struct EditMsg {
    content: String,
}

impl TryFrom<Msg> for RealMsg {
    type Error = std::num::ParseIntError;

    fn try_from(value: Msg) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id.parse()?,
            channel_id: value.channel_id.parse()?,
            content: value.content,
        })
    }
}

#[derive(serde::Deserialize)]
struct Msg {
    id: String,
    channel_id: String,
    content: String,
}

#[derive(Debug)]
struct RealMsg {
    id: i64,
    channel_id: i64,
    content: String,
}

#[derive(serde::Deserialize)]
struct MessageResponse {
    total_results: i64,
    messages: Vec<(Msg,)>,
}

#[derive(Debug, serde::Deserialize, Clone)]
struct Article {
    title: String,
    extract: String,
}

#[derive(serde::Deserialize)]
struct ArticleReq {
    query: ArticleReqQuery,
}

#[derive(serde::Deserialize)]
struct ArticleReqQuery {
    pages: Vec<Article>,
}

async fn get_article(client: &Client) -> anyhow::Result<Article> {
    let article_list: ArticleList = client
        .get("https://en.uncyclopedia.co/w/api.php")
        .query(&[
            ("action", "query"),
            ("format", "json"),
            ("list", "random"),
            ("rnlimit", "1"),
            ("rnnamespace", "0"),
        ])
        .send()
        .await?
        .json()
        .await?;
    let article = article_list
        .query
        .random
        .first()
        .context("no random articles found")?;
    let article_req: ArticleReq = client
        .get("https://en.uncyclopedia.co/w/api.php")
        .query(&[
            ("action", "query"),
            ("prop", "extracts"),
            ("exsentences", "10"),
            ("exlimit", "1"),
            ("titles", &article.title),
            ("explaintext", "1"),
            ("formatversion", "2"),
            ("format", "json"),
        ])
        .send()
        .await?
        .json()
        .await?;
    Ok(article_req
        .query
        .pages
        .first()
        .cloned()
        .context("article not found")?)
}

#[derive(serde::Deserialize)]
struct ArticleList {
    query: ArticleQuery,
}
#[derive(serde::Deserialize)]
struct ArticleQuery {
    random: Vec<ArticleListing>,
}
#[derive(serde::Deserialize)]
struct ArticleListing {
    id: i32,
    title: String,
}

#[tokio::test]
async fn test_artcle() {
    let client = Client::new();
    let article = get_article(&client).await.unwrap();
    println!("{:?}", article);
}
