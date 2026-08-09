REPORT z_demo.

FORM greet USING lv_name TYPE string.
  DATA(lv_msg) = |Hello, { lv_name }!|.
  WRITE: lv_msg.
ENDFORM.

FORM add_numbers USING a TYPE i b TYPE i CHANGING result TYPE i.
  result = a + b.
ENDFORM.
