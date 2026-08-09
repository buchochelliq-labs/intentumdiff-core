SELECT id, name, 42 AS answer
FROM users
WHERE age > 18
ORDER BY name;

INSERT INTO logs (message) VALUES ('Hello, World!');

UPDATE users SET active = 1 WHERE id = 7;
