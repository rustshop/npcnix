variable "bucket" {
  description = "s3 bucket to use to store NixOs configs in"
}

variable "region" {
  description = "s3 region to use to store NixOs configs in"
}

variable "prefix" {
  type        = string
  description = "prefix to give access to ec2 machines"
}

output "bucket" {
  value = var.bucket
}

output "region" {
  value = var.region
}

output "prefix" {
  value = var.prefix
}

output "iam_policy" {
  value = aws_iam_policy.iam_policy
}

resource "aws_iam_policy" "iam_policy" {
  path        = "/"
  description = "Allow Nix System Config access by EC2 machines with the right profile"

  policy = jsonencode({
    "Version" : "2012-10-17",
    "Statement" : [
      {
        "Sid" : "ConfigNixosAccess",
        "Effect" : "Allow",
        "Action" : [
          "s3:GetObject",
          "s3:GetObjectMetaData",
          "s3:GetObjectAttributes",
        ],
        "Resource" : [
          "arn:aws:s3:::${var.bucket.id}/${var.prefix}/*"
        ]
      }
    ]
  })
}
