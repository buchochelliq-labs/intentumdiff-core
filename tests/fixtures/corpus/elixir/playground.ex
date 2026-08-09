defmodule Greeter do
  def greet(name) do
    "Hello, " <> name
  end

  def farewell(name) do
    "Goodbye, " <> name
  end

  def shout(phrase) do
    String.upcase(phrase)
  end
end
