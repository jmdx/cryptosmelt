CREATE ROLE cryptosmelt LOGIN PASSWORD 'cryptosmeltpw';
CREATE SCHEMA cryptosmelt;
CREATE DATABASE cryptosmelt;
GRANT ALL ON SCHEMA cryptosmelt TO cryptosmelt;
GRANT ALL ON ALL TABLES IN SCHEMA cryptosmelt TO cryptosmelt;
