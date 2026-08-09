Module HelloWorld
    Sub Greet(name As String)
        Console.WriteLine($"Hello, {name}!")
    End Sub

    Function Add(a As Integer, b As Integer) As Integer
        Return a + b
    End Function

    Function Multiply(a As Integer, b As Integer) As Integer
        Return a * b
    End Function
End Module
