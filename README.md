# Fetche

Fetche allows you to monitor HTTP endpoints with minimal setup.

## Setup

1. Build the dockerfile
2. Define a fetche.toml, see example.toml for guidance
3. Come up with a location for the sqlite database file, then write down the database url in the sqlite connection string format
   - if the database doesn't exist yet, add ?mode=rwc to the end of the string
     for example, if you want to store the database in this directory in a file called db.sqlite3,
     the connection string would look like this: `sqlite://db.sqlite3?mode=rwc`
4. Start fetche:

Substitute `$DATABASE_URL` for the connection string you came up with.

```bash
docker run -e DATABASE_URL=$DATABASE_URL -p 8010:8080 -v ./fetche.toml:/app/fetche.toml -it --rm fetche
```

Press `Ctrl + C` to stop.

## Usage

Once Fetche is up and fetching data, you probably want some to access to it.
Fetche comes with an inbuilt http api, but you need to expose that explicitly when using docker. The `-p 8010:8080` option defined above allows us to map a port (8080) inside the container to any port on your computer (for example 8010).

Opening `http://localhost:8010/` in your browser should give you a list of all configs you have defined in your fetche.toml. Configs that are present in the latest version are marked with `active: true`. Note that the return order of the configs is randomized.

`/query_list` returns a list of all recorded events, such as:

- The endpoint returned new data
  - `status.code` contains the http status
  - `status.tag` might be one of
    - `HttpOk`
    - `HttpErr`
  - `data` contains one of
    - `json` if try_parse_json was set and the endpoint returned valid json
    - `plain_text` otherwise
- Fetche was down at the time, but according to the config a fetch should've been performed at "fetched_at"
  - `status.tag` is `Unknown`
- An error occurred while trying to fetch source_url
  - `status.tag` is `Error`

There are some query options you can set:

- filter_config=SOME_HASH: only return events from config with hash SOME_HASH
- decompress=true|false: generate datapoints for time periods when nothing changed, by default false
  - generated datapoints are marked with `from_db: false`

For example: `http://localhost:8010/query_list?filter_config=10038156192638179075&decompress=true` (You don't have a config with that hash)

`/query` behaves exactly like `/query_list`, but the results are grouped by config.
