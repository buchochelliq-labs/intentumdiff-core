resource "aws_instance" "web" {
  ami           = "ami-0c55b159cbfafe1f0"
  instance_type = "t3.small"
  backup_window  = "03:00-04:30"

  tags = {
    Name        = "web-server"
    Environment = "production"
  }
}

output "instance_ip" {
  value = aws_instance.web.public_ip
}
