CREATE OR REPLACE PROCEDURE greet(p_name IN VARCHAR2) IS
  v_msg VARCHAR2(100);
BEGIN
  v_msg := 'Hello, ' || p_name || '!';
  DBMS_OUTPUT.PUT_LINE(v_msg);
END greet;
/

CREATE OR REPLACE FUNCTION add_nums(p_a IN NUMBER, p_b IN NUMBER)
  RETURN NUMBER IS
BEGIN
  RETURN p_a + p_b;
END add_nums;
/

CREATE OR REPLACE FUNCTION multiply_nums(p_a IN NUMBER, p_b IN NUMBER)
  RETURN NUMBER IS
BEGIN
  RETURN p_a * p_b;
END multiply_nums;
/
