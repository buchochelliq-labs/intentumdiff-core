CREATE VIEW active_users AS
SELECT id, name FROM users WHERE active = 1;

CREATE VIEW recent_orders AS
SELECT id, total FROM orders WHERE placed_at > '2026-01-01';

CREATE VIEW dormant_accounts AS
SELECT id FROM accounts WHERE last_login < '2025-06-30';
