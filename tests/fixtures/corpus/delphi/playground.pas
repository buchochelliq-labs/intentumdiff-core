program Demo;

procedure Greet(const Name: string);
begin
  WriteLn(Format('Hello, %s!', [Name]));
end;

function Add(const A, B: Integer): Integer;
begin
  Result := A + B;
end;

function Multiply(const A, B: Integer): Integer;
begin
  Result := A * B;
end;

begin
  Greet('World');
  WriteLn(Add(2, 3));
  WriteLn(Multiply(2, 3));
end.
