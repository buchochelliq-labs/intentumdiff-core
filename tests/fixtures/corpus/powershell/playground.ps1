function Greet {
    param(
        [string]$Name = 'World'
    )
    Write-Host "Hello, $Name!"
}

function Add-Numbers {
    param(
        [int]$A,
        [int]$B
    )
    return $A + $B
}

function Multiply-Numbers {
    param(
        [int]$A,
        [int]$B
    )
    return $A * $B
}
