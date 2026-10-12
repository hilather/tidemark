# W1-A parser fixtures

These are synthetic, recorded-format examples, not claimed captures from Rocky VMs. No VM is available in this attempt. The four mountinfo files cover EL8-style XFS/LVM, md, bind mounts, stacked overmounts, optional propagation fields and octal escapes including non-UTF8 path bytes. Obtain genuine Rocky 8.8 and 8.10 captures during VM validation.

Sysfs topology and 11/15/17-field stat examples are materialized by `blockdev.rs` tests in temporary directories, including dm → md → partition → whole-disk traversal, duplicate leaves, malformed counters and unavailable queue attributes.
