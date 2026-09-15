-- Minimal catalog mirroring the real ETI game.db schema. Keys are synthetic.
CREATE TABLE "genre" (`genre_id` INTEGER, `genre_de` TEXT, `genre_en` TEXT, `genre_fr` TEXT, PRIMARY KEY(`genre_id`));
CREATE TABLE "games" (`game_id` TEXT, `db_id` INTEGER, `game_title` TEXT, `game_key` TEXT, `game_release` TEXT,
  `game_publisher` TEXT, `game_size` NUMERIC, `game_readme_de` TEXT, `game_readme_en` TEXT, `game_readme_fr` TEXT,
  `game_maxplayers` INTEGER, `game_master_req` INTEGER, `genre_id` INTEGER, `game_version` TEXT, PRIMARY KEY(`db_id`));
CREATE TABLE "discarded" (`del_id` INTEGER, `game_id` TEXT, `game_key` INTEGER, PRIMARY KEY(`del_id`));
CREATE TABLE "tools" ("tool_id" TEXT, "db_id" INTEGER, "tool_name" TEXT, "tool_key" TEXT, "tool_maintainer" TEXT,
  "tool_size" TEXT, "tool_readme_de" TEXT, "tool_readme_en" TEXT, "tool_readme_fr" TEXT, "tool_disabled" INTEGER, PRIMARY KEY("db_id"));

INSERT INTO genre VALUES (1,'Echtzeit-Strategie','Real-time strategy','Stratégie en temps réel');
INSERT INTO genre VALUES (2,'Ego-Shooter','First-person Shooter','First-person shooter');

INSERT INTO games VALUES ('quake3', 1, 'Quake 3 Arena', 'BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', '1999', 'id Software', 0.91,
  'Der Klassiker.<br>Arena-Shooter.', 'The classic arena shooter.', NULL, 16, 0, 2, '20160922');
INSERT INTO games VALUES ('bfbc2', 2, 'Battlefield: Bad Company 2', 'BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB', '2010', 'EA DICE', 16,
  'Braucht Masterserver.', 'Needs master server.', NULL, 32, 1, 2, '20210416');
INSERT INTO games VALUES ('weird', 3, 'Weird Players Game', 'BCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC', '2005', NULL, 1.5,
  NULL, NULL, NULL, '2 (+ 6 CPU)', 0, 1, '20200101');
INSERT INTO games VALUES ('amongus', 4, 'Among Us', 'BDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD', '2018', 'Innersloth', 0.3,
  NULL, NULL, NULL, 15, 0, NULL, '20250308');
-- write key must be rejected
INSERT INTO games VALUES ('badkey', 5, 'Bad Key', 'AEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE', NULL, NULL, 1, NULL, NULL, NULL, 4, 0, 1, '20200101');
-- path traversal id must be rejected
INSERT INTO games VALUES ('../evil', 6, 'Evil', 'BFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF', NULL, NULL, 1, NULL, NULL, NULL, 4, 0, 1, '20200101');

INSERT INTO discarded VALUES (1, 'sc2', 'BGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG');

INSERT INTO tools VALUES ('eti_lanshare', 1, 'LANshare 3', 'BHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH', 'eti Team', '0.2', NULL, NULL, NULL, NULL);
INSERT INTO tools VALUES ('eti_lansteam', 2, 'LAN Steam', 'BIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII', 'eti Team', '0', NULL, NULL, NULL, 1);
