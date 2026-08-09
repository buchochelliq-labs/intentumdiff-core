program Demo;

procedure Greet(Name: string);
begin
  WriteLn('Hello, ' + Name);
end;

function Add(A, B: Integer): Integer;
begin
  Result := A + B;
end;

begin
  Greet('World');
  WriteLn(Add(1, 41));
end.
