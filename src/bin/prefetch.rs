//! Prefetch export windows prefetch file to json
extern crate tap_plugin_prefetch;

use std::env;
use std::fs::File;
use std::sync::Arc;
use std::io::BufReader;

use tap::value::Value;
use tap_plugin_prefetch::Prefetch;

fn main() 
{
   if env::args().len() != 2 
   {
     println!("prefetch input_file");
     return ;
   }

   let args: Vec<String> = env::args().collect();
   let file_path = &args[1];

   match File::open(file_path)
   {
      Err(_) => println!("Can't open file {}", file_path),
      Ok(file) => 
      {
         let mut buffered = BufReader::new(file);
         let prefetch_parser = match Prefetch::from_file(&mut buffered)
         {
           Ok(prefetch_parser) => prefetch_parser,
           Err(err) => {eprintln!("{}", err); return },
         };
      
         let value : Value = Value::ReflectStruct(Arc::new(prefetch_parser));
         println!("{}", serde_json::to_string(&value).unwrap());
      },
   }
}
