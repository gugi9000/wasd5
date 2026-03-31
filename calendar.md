# Family calendar

3 columns

- Randi
- Bjarke
- Fælles

Default view should show three columns starting two days before today.

        | Randi  | Bjarke | Fælles
March
søn. 29 | Foo    |        |
man. 30 |        | Bar    |
tir. 31 |        |        | Strength training

etc.

Each appointment should be editable by clicking it
A button to create new appointment should be on top

Edit using a toast popup is probably most ergonomic
Start and end time are optional


It should be possible to scroll back and forward
- either through pagination or dynamic refresh

At least one month ahead should be shown as default


Appoints should be stored in the exsisting sqlite-database using Diesel-rs
