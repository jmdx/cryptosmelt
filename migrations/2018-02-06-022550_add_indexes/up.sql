CREATE INDEX ON valid_share
  (((date_trunc('hour'::text, created)
    + ((((date_part('minute'::text, created))::integer / 5))::double precision
    * '00:05:00'::interval))), miner_alias);
CREATE INDEX ON valid_share (created);