-- Demo catalog with the real ETI schema and synthetic keys (no real shares).
CREATE TABLE "genre" (`genre_id` INTEGER, `genre_de` TEXT, `genre_en` TEXT, `genre_fr` TEXT, PRIMARY KEY(`genre_id`));
CREATE TABLE "games" (`game_id` TEXT, `db_id` INTEGER, `game_title` TEXT, `game_key` TEXT, `game_release` TEXT,
  `game_publisher` TEXT, `game_size` NUMERIC, `game_readme_de` TEXT, `game_readme_en` TEXT, `game_readme_fr` TEXT,
  `game_maxplayers` INTEGER, `game_master_req` INTEGER, `genre_id` INTEGER, `game_version` TEXT, PRIMARY KEY(`db_id`));
CREATE TABLE "discarded" (`del_id` INTEGER, `game_id` TEXT, `game_key` INTEGER, PRIMARY KEY(`del_id`));
CREATE TABLE "tools" ("tool_id" TEXT, "db_id" INTEGER, "tool_name" TEXT, "tool_key" TEXT, "tool_maintainer" TEXT,
  "tool_size" TEXT, "tool_readme_de" TEXT, "tool_readme_en" TEXT, "tool_readme_fr" TEXT, "tool_disabled" INTEGER, PRIMARY KEY("db_id"));
INSERT INTO genre VALUES (1,'Echtzeit-Strategie','Real-time strategy','Stratégie en temps réel');
INSERT INTO genre VALUES (2,'Ego-Shooter','First-person Shooter','First-person shooter');
INSERT INTO genre VALUES (6,'Racing','Racing','Courses');
INSERT INTO genre VALUES (12,'Sport','Sports','Sportif');
INSERT INTO genre VALUES (14,'Casual','Casual','Casual');
INSERT INTO genre VALUES (5,'Survival','Survival','Survie');
INSERT INTO games VALUES ('amongus', 1, 'Among Us', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA2', '2018', 'Innersloth', 0.45, 'Wer ist der Impostor? Bis zu 15 Spieler im lokalen Netzwerk.', 'Who is the impostor? Up to 15 players on the local network.', NULL, 15, 0, 14, '20250308');
INSERT INTO games VALUES ('quake3', 2, 'Quake 3 Arena', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA3', '1999', 'id Software', 0.91, 'Der Arena-Shooter-Klassiker.<br>Läuft auf jedem Toaster.', 'The classic arena shooter.', NULL, 16, 0, 2, '20160922');
INSERT INTO games VALUES ('cod4', 3, 'Call of Duty 4: Modern Warfare', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA4', '2007', 'Activision', 8.2, 'LAN-Klassiker mit Promod-Support.', 'LAN classic with promod support.', NULL, 32, 0, 2, '20220110');
INSERT INTO games VALUES ('wc3', 4, 'Warcraft III: The Frozen Throne', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA5', '2003', 'Blizzard Entertainment', 1.6, 'Inklusive DotA und Tower-Defense-Maps.', 'Includes DotA and tower defense maps.', NULL, 12, 0, 1, '20201021');
INSERT INTO games VALUES ('rocket', 5, 'Rocket League', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA6', '2015', 'Psyonix', 7.1, 'Autoball mit Raketenantrieb.', 'Soccer with rocket-powered cars.', NULL, 8, 0, 12, '20260410');
INSERT INTO games VALUES ('goldsrc', 6, 'Counter-Strike 1.6 (GoldSrc)', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAA7', '2003', 'Valve', 1.0, 'CS 1.6, CS 1.5 und Half-Life in einem Paket.', 'CS 1.6, CS 1.5 and Half-Life in one package.', NULL, 32, 0, 2, '20240623');
INSERT INTO games VALUES ('l4d2', 7, 'Left 4 Dead 2', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB2', '2009', 'Valve', 10.0, 'Koop-Zombie-Shooter für 4 Spieler (8 im Versus).', 'Co-op zombie shooter for 4 players (8 in versus).', NULL, 8, 0, 2, '20240115');
INSERT INTO games VALUES ('factorio', 8, 'Factorio', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB3', '2020', 'Wube Software', 2.3, 'Die Fabrik muss wachsen.', 'The factory must grow.', NULL, 65535, 0, 1, '20250801');
INSERT INTO games VALUES ('flat2', 9, 'FlatOut 2', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB4', '2006', 'Bugbear', 3.1, 'Zerstörungsrennen mit Ragdoll-Physik.', 'Destruction racing with ragdoll physics.', NULL, 8, 0, 6, '20190502');
INSERT INTO games VALUES ('bfbc2', 10, 'Battlefield: Bad Company 2', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB5', '2010', 'EA DICE', 16, 'Benötigt den Battlefield-Masterserver im LAN.', 'Needs the Battlefield master server on the LAN.', NULL, 32, 1, 2, '20210416');
INSERT INTO games VALUES ('7days', 11, '7 Days to Die', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB6', '2013', 'The Fun Pimps', 12.5, 'Survival-Crafting mit Zombies.', 'Survival crafting with zombies.', NULL, 8, 0, 5, '20241201');
INSERT INTO games VALUES ('aoe2', 12, 'Age of Empires II: HD', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAB7', '2013', 'Microsoft', 4.4, 'Wololo.', 'Wololo.', NULL, 8, 0, 1, '20230917');
INSERT INTO games VALUES ('trackmania', 13, 'TrackMania Nations Forever', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAC2', '2008', 'Nadeo', 0.6, 'Kostenloser Arcade-Racer, perfekt für Zwischendurch.', 'Free arcade racer, perfect between rounds.', NULL, 200, 0, 6, '20180303');
INSERT INTO games VALUES ('supra', 14, 'Supraball', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAC3', '2016', 'Supra Games', 3.0, 'Fußball aus der Ego-Perspektive.', 'First-person football.', NULL, 10, 0, 12, '20200606');
INSERT INTO tools VALUES ('eti_lanshare', 1, 'LANshare 3', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAD2', 'eti Team', '0.2', NULL, NULL, NULL, NULL);
INSERT INTO tools VALUES ('eti_lanpage', 2, 'LANPage', 'BDEMO2AAAAAAAAAAAAAAAAAAAAAAAAAD3', 'eti Team', '0.1', NULL, NULL, NULL, NULL);
