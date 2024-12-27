-- Add up migration script here
CREATE TABLE "config" (
	hash											integer UNIQUE PRIMARY KEY NOT NULL,
	source_url								text NOT NULL,
	fetch_interval_s					integer NOT NULL,
	try_parse_json						integer NOT NULL, -- boolean
	active										integer NOT NULL, -- boolean
	last_fetched							integer -- seconds since unix epoch
);

CREATE TABLE "fetch_result" (
	config										integer NOT NULL REFERENCES config(hash),
	fetched_at								integer NOT NULL, -- seconds since unix epoch
	created_at								integer NOT NULL, -- seconds since unix epoch
	source_url								text NOT NULL,
	status										text NOT NULL, -- See the "status" enum
	body_text									text,
	valid_json								bool -- boolean if config.try_parse_json was true
);
