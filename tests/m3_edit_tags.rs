//! M3.5 integration tests — EditTags / HasGenres / HasCollections
//! emit the right `field[i].tag.tag=v` / `field[].tag.tag-=csv`
//! wire form.

use plex_rs::{HasCollections, HasGenres, PlexServer, PlexToken};
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn root_body() -> serde_json::Value {
    serde_json::json!({"MediaContainer": {"size": 0, "machineIdentifier": "m", "version": "v"}})
}
fn sections_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Directory": [{"key": "1", "type": "movie", "title": "Movies"}]
        }
    })
}
fn movies_body() -> serde_json::Value {
    serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Metadata": [{
                "ratingKey": "100", "key": "/library/metadata/100", "title": "X"
            }]
        }
    })
}

async fn setup() -> (MockServer, plex_rs::Movie) {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(root_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sections_body()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/library/sections/1/all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(movies_body()))
        .mount(&server)
        .await;
    let plex = PlexServer::connect(
        Url::parse(&server.uri()).unwrap(),
        PlexToken::new("tok").unwrap(),
    )
    .await
    .unwrap();
    let movie = plex.library().sections().await.unwrap()[0]
        .movies()
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    (server, movie)
}

#[tokio::test]
async fn replace_genres_emits_indexed_tag_tag_pairs() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("type", "1"))
        // wiremock unescapes %5B/%5D for matching, so the keys come
        // through as bare brackets.
        .and(query_param("genre[0].tag.tag", "Action"))
        .and(query_param("genre[1].tag.tag", "Sci-Fi"))
        .and(query_param("genre.locked", "1"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie
        .replace_genres(&["Action", "Sci-Fi"], true)
        .await
        .unwrap();
}

#[tokio::test]
async fn remove_genres_uses_csv_with_remove_sigil() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("id", "100"))
        .and(query_param("genre[].tag.tag-", "Comedy,Romance"))
        .and(query_param("genre.locked", "0"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie
        .remove_genres(&["Comedy", "Romance"], false)
        .await
        .unwrap();
}

#[tokio::test]
async fn replace_collections_uses_collection_field() {
    let (server, movie) = setup().await;
    Mock::given(method("PUT"))
        .and(path("/library/sections/1/all"))
        .and(query_param("collection[0].tag.tag", "Best of 2024"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    movie
        .replace_collections(&["Best of 2024"], true)
        .await
        .unwrap();
}
